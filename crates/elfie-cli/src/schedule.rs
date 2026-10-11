// @lfy def/cli/main.lfy:main
//! The schedule of one round of a compile: which batches may start, how many run at once,
//! where each one runs, what it takes from the batches before it, and what becomes of it.

use super::*;
use crate::copies::{Change, Changes, Kept, KeptFile, Mirror, Where, kept_file, merge_changes, merged};
use crate::processes::Runs;
use crate::running::Run;

/// How a batch ended, as the schedule reads it.
// @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
pub(crate) enum Done {
    /// Its work is in the root: nothing that depends on it waits any longer.
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    Merged,
    /// A file of it could not be merged without a conflict, so it runs again from a new copy
    /// once no other batch is running; this is not a rejection.
    // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
    Conflicted,
    /// It stopped without its files reaching the root, so a batch whose copy took its
    /// accepted changes stops too.
    // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
    Stopped,
}

/// What has become of one batch of a round.
// @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// It has not started, or it is to run again.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    Waiting,
    /// It is running.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    Running,
    /// Every unit of it was accepted by `accept`, and it is neither running the compiler nor
    /// about to run it again; its files are not in the root yet.
    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
    Settled,
    /// Its files are in the root.
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    Merged,
    /// It stopped without its files reaching the root.
    // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
    Stopped,
}

/// Which batches of a round may start: a batch starts once every batch holding a dependency
/// of one of its units is done, at most --jobs run at once, and among those that can start
/// they start in plan order.
// @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
pub(crate) struct Planned {
    pub(crate) batch: Batch,
    /// The places in the round of the batches holding a dependency of one of its units.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    pub(crate) after: Vec<usize>,
}

/// What a round of a compile has started, finished, and has left to start.
// @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
#[derive(Default)]
pub(crate) struct Schedule {
    /// The places of the batches not started yet, in plan order.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    waiting: Vec<usize>,
    running: usize,
    /// What has become of each batch of the round, by its place.
    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
    states: Vec<State>,
    /// The accepted changes of each batch that has any: each file of its copy that differs
    /// from its base, was added, or was removed, with what the copy held for it the last time
    /// every unit of the batch was accepted.
    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
    accepted: BTreeMap<usize, Changes>,
    /// The places of the batches that must run with nothing else, because a file of theirs
    /// could not be merged.
    // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
    alone: BTreeSet<usize>,
    /// Whether a batch that must run with nothing else is running.
    // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
    exclusive: bool,
}

/// What a round does next.
// @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
pub(crate) enum Next {
    /// Start the batch at this place.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    Start(usize),
    /// Wait for a running batch to finish.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    Wait,
    /// Nothing is running and nothing can start.
    // @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
    Over,
}

impl Schedule {
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    pub(crate) fn of(planned: &[Planned]) -> Schedule {
        Schedule {
            waiting: (0..planned.len()).collect(),
            // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
            states: vec![State::Waiting; planned.len()],
            ..Schedule::default()
        }
    }

    /// Whether a batch a waiting one depends on has come far enough to let it start: its
    /// files are in the root, or it stopped, so nothing of it will ever reach the root.
    ///
    /// With copies, a batch whose every unit was accepted and that is neither running the
    /// compiler nor about to run it again is far enough too: what it has accepted no longer
    /// changes, so a copy may take it before its review ends and its files are merged.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
    pub(crate) fn far_enough(&self, at: usize, copies: bool) -> bool {
        match self.states.get(at).copied().unwrap_or(State::Waiting) {
            // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
            State::Merged | State::Stopped => true,
            // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
            State::Settled => copies,
            State::Waiting | State::Running => false,
        }
    }

    /// The batch to start, in plan order among those that can: one whose every batch holding
    /// a dependency of one of its units, directly or through the units of other batches, has
    /// come far enough, while fewer than --jobs are running.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
    pub(crate) fn next(&mut self, planned: &[Planned], jobs: usize, halted: bool, copies: bool) -> Next {
        if self.waiting.is_empty() || halted {
            // The compile stopped: no batch starts after that, and the batches already
            // running finish as they would.
            // @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
            return if self.running == 0 { Next::Over } else { Next::Wait };
        }
        // At most --jobs batches are running the compiler, the verifier, or a command of the
        // target at once, since a batch runs one of them from the moment it starts until it
        // is done.
        // @lfy def/cli/main.lfy:main#main:main:dea9d85906c909cb3fc6dbc07d671a677ca474f87b64441b5bbd26a79c78c24b
        if self.running >= jobs || self.exclusive {
            return Next::Wait;
        }
        // Among the batches that can start, they start in plan order.
        // @lfy def/cli/main.lfy:main#main:main:97db05bc4c7806f4b6fdeff9a55295d8abf986349e700879098a18335620e22a
        let found = self.waiting.iter().position(|&at| {
            // A batch that must run with nothing else waits until nothing else is running.
            // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
            if self.alone.contains(&at) && self.running > 0 {
                return false;
            }
            // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
            // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
            planned[at].after.iter().all(|&waited| self.far_enough(waited, copies))
        });
        match found {
            Some(place) => {
                let at = self.waiting.remove(place);
                self.running += 1;
                // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
                self.states[at] = State::Running;
                // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
                self.exclusive = self.alone.contains(&at);
                Next::Start(at)
            }
            // Nothing can start: either a running batch will let one, or the batches left
            // wait on a batch that never finished.
            // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
            None if self.running > 0 => Next::Wait,
            // @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
            None => Next::Over,
        }
    }

    /// One batch's every unit accepted and nothing about to run the compiler on it again: its
    /// accepted changes are what a copy of a batch depending on it takes from here.
    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
    pub(crate) fn settled(&mut self, at: usize, changes: Changes) {
        // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
        self.accepted.insert(at, changes);
        // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
        if self.states[at] == State::Running {
            self.states[at] = State::Settled;
        }
    }

    /// A batch that was done running again: it is no longer done, and nothing may take it as
    /// far enough until it settles once more.
    // @lfy def/cli/main.lfy:main#main:main:107c61d983a8522a99a186d5c8b3e788d5407f1e171398cf2aa4910fa72a7a30
    pub(crate) fn runs_again(&mut self, at: usize) {
        // @lfy def/cli/main.lfy:main#main:main:107c61d983a8522a99a186d5c8b3e788d5407f1e171398cf2aa4910fa72a7a30
        self.accepted.remove(&at);
        self.states[at] = State::Running;
    }

    /// One batch finished: done, stopped, or waiting to run again from a new copy with
    /// nothing else running.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    pub(crate) fn finish(&mut self, at: usize, done: &Done) {
        self.running -= 1;
        self.exclusive = false;
        match done {
            // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
            Done::Merged => {
                self.states[at] = State::Merged;
            }
            // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
            Done::Stopped => {
                self.states[at] = State::Stopped;
                self.accepted.remove(&at);
            }
            // It runs again from a new copy once no other batch is running; its place goes
            // back among those waiting, in plan order.
            // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
            Done::Conflicted => {
                self.alone.insert(at);
                self.states[at] = State::Waiting;
                self.accepted.remove(&at);
                let place = self.waiting.iter().position(|&other| other > at).unwrap_or(self.waiting.len());
                self.waiting.insert(place, at);
            }
        }
    }

    /// The batches of a round that never ran, because what they wait on never finished.
    // @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
    pub(crate) fn unstarted(&self) -> Vec<usize> {
        self.waiting.clone()
    }
}

/// For each batch of a round, the places of the batches holding a dependency of one of its
/// units, directly or through the units of other batches.
// @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
// @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
pub(crate) fn round_of(plan: &Plan, batches: &[usize]) -> Vec<Planned> {
    let mut planned: Vec<Planned> = Vec::new();
    for &index in batches {
        let batch = plan.batches[index].clone();
        // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
        let needed: BTreeSet<usize> =
            batch.units.iter().flat_map(|&unit| plan.units[unit].dependencies.iter().copied()).collect();
        // A batch holds a dependency of one of its units directly, or through the units of
        // the batches that one depends on, which are all earlier in plan order.
        // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
        let mut after: BTreeSet<usize> = BTreeSet::new();
        for (at, earlier) in planned.iter().enumerate() {
            if earlier.batch.units.iter().any(|unit| needed.contains(unit)) {
                after.insert(at);
                // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
                after.extend(earlier.after.iter().copied());
            }
        }
        planned.push(Planned { batch, after: after.into_iter().collect() });
    }
    planned
}

/// What a batch's thread needs of the round it runs in: the schedule, what wakes it when
/// another batch moves on, where it sits, and whether batches run in copies.
// @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
pub(crate) struct Round<'r> {
    pub(crate) schedule: &'r Mutex<Schedule>,
    /// What a batch waits on while another batch merges or stops.
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    pub(crate) finished: &'r Condvar,
    pub(crate) planned: &'r [Planned],
    /// The batch's own place in the round.
    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
    pub(crate) at: usize,
    /// Whether a batch runs in a copy of the root rather than in the root itself.
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    pub(crate) copies: bool,
}

impl Round<'_> {
    /// The schedule, locked.
    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
    pub(crate) fn held(&self) -> std::sync::MutexGuard<'_, Schedule> {
        self.schedule.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The accepted changes of each batch holding a dependency of one of this batch's units,
    /// directly or through the units of other batches, that does not have its files in the
    /// root, in plan order: what this batch's copy takes when it is made.
    // @lfy def/cli/main.lfy:main#main:main:f3bb66e88d43cf4578514a8d298ea474abb5cefc6ca529992953e31d189978e8
    pub(crate) fn to_take(&self) -> Vec<(usize, Changes)> {
        let held = self.held();
        // @lfy def/cli/main.lfy:main#main:main:f3bb66e88d43cf4578514a8d298ea474abb5cefc6ca529992953e31d189978e8
        self.planned[self.at]
            .after
            .iter()
            .filter(|&&waited| held.states[waited] != State::Merged)
            .filter_map(|&waited| held.accepted.get(&waited).map(|changes| (waited, changes.clone())))
            .collect()
    }

    /// Waits until every batch whose accepted changes this batch's copy took has its files in
    /// the root or has stopped; whether one of them stopped without them reaching it.
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    // @lfy def/cli/main.lfy:main#main:main:f5720ad6fd831e736d050c0b20f67ced9fd4c44fe185806606e36c8d267767fc
    // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
    pub(crate) fn wait_for(&self, took: &[usize]) -> bool {
        let mut held = self.held();
        loop {
            // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
            if took.iter().any(|&waited| held.states[waited] == State::Stopped) {
                return true;
            }
            // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
            if took.iter().all(|&waited| held.states[waited] == State::Merged) {
                return false;
            }
            // @lfy def/cli/main.lfy:main#main:main:f5720ad6fd831e736d050c0b20f67ced9fd4c44fe185806606e36c8d267767fc
            held = self.finished.wait(held).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// Waits until the batch at one place has its files in the root, or has stopped.
    // @lfy def/cli/main.lfy:main#main:main:49481a50bf94e617c823f0536f8d44cdb0ca79898e0c7d84e0bd7ac9070d5960
    pub(crate) fn wait_for_one(&self, waited: usize) {
        let mut held = self.held();
        // @lfy def/cli/main.lfy:main#main:main:49481a50bf94e617c823f0536f8d44cdb0ca79898e0c7d84e0bd7ac9070d5960
        while held.states[waited] != State::Merged && held.states[waited] != State::Stopped {
            held = self.finished.wait(held).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// The batch's every unit accepted, and nothing about to run the compiler on it again.
    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
    pub(crate) fn settled(&self, changes: Changes) {
        // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
        self.held().settled(self.at, changes);
        self.finished.notify_all();
    }

    /// A batch that was done running again: it is no longer done.
    // @lfy def/cli/main.lfy:main#main:main:107c61d983a8522a99a186d5c8b3e788d5407f1e171398cf2aa4910fa72a7a30
    pub(crate) fn runs_again(&self) {
        // @lfy def/cli/main.lfy:main#main:main:107c61d983a8522a99a186d5c8b3e788d5407f1e171398cf2aa4910fa72a7a30
        self.held().runs_again(self.at);
        self.finished.notify_all();
    }
}

/// The changes a batch has made where it runs: nothing at all when it runs in the root, which
/// is also where nothing takes another batch's work.
// @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
pub(crate) fn changes_of(where_: &Where) -> Changes {
    match where_ {
        // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
        Where::Copy(mirror) => mirror.changes(),
        Where::Root(_) => Changes::new(),
    }
}

/// What one run of a batch in one directory came to: it is over, or the batch runs again from
/// a new copy with what this one held as its kept work.
// @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
pub(crate) enum Attempt {
    Over(Done),
    Again(Kept),
}

/// One batch run from beginning to end, and once more from a new copy where the work of a
/// batch it took changes from reached the root mixed with other files.
// @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
pub(crate) fn run_batch(run: &Mutex<Run>, batch: &Batch, command: &str, root: &Path, round: &Round) -> Done {
    // The work the last compile of this batch kept, when a file beside the root holds work of
    // the batch's own that a run rejected and nothing accepted.
    // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
    let mut kept = kept_work(run, batch, root);
    loop {
        match run_in_copy(run, batch, command, root, round, kept.as_ref()) {
            // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
            Attempt::Over(done) => return done,
            // The batch runs again from a new copy, which takes its changes as kept work is
            // taken, and its runs of the compiler and the verifier are counted afresh.
            // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
            // @lfy def/cli/main.lfy:main#main:main:107c61d983a8522a99a186d5c8b3e788d5407f1e171398cf2aa4910fa72a7a30
            Attempt::Again(again) => {
                round.runs_again();
                kept = Some(again);
            }
        }
    }
}

/// One run of a batch in one directory: its own copy of the root where there is one, seeded
/// with the accepted changes of the batches it depends on, its kept work taken, the compiler,
/// the verifier, and then the merge of its work into the root.
///
/// A batch whose work passed has every file of its copy merged into the root, its source maps
/// recorded there, and its copy removed. A batch that stopped merges nothing and leaves its
/// copy in place for a person to read.
// @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
pub(crate) fn run_in_copy(
    run: &Mutex<Run>,
    batch: &Batch,
    command: &str,
    root: &Path,
    round: &Round,
    kept: Option<&Kept>,
) -> Attempt {
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    let (mut where_, took) = match make_copy(run, batch, root, round) {
        Some(made) => made,
        // The batch stops as a batch that depends on a stopped batch does.
        // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
        None => {
            let mut state = locked(run);
            state.stop(batch);
            state.worsen(ExitCode::Problems);
            return Attempt::Over(Done::Stopped);
        }
    };
    // The kept work taken into the copy, after the base was kept.
    // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
    let took_kept = take_kept(run, batch, &where_, kept);
    let runs = {
        let mut state = locked(run);
        let agent = state.agent.clone();
        Runs::new(agent, state.running_of(&batch.identifier))
    };
    // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
    let compiled = Run::compile_batch(run, batch, command, &where_, &runs, took_kept.as_ref(), round);
    let Where::Copy(mirror) = &mut where_ else {
        // The batch ran in the root: there is nothing to merge, and its source maps are
        // recorded where they already are. No kept work is written or taken for it, because
        // a rejected batch's outputs are left on disk in the root.
        // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
        // @lfy def/cli/main.lfy:main#main:main:8444ea8a2d9ff6b491b88ffdab12f7f13f9596b9dfd77f20d2a51a121c7cad69
        if let Some(held) = &compiled.held {
            // @lfy def/cli/main.lfy:main#main:main:5841e3d28e75b69d42ed2ba411b34aa9429ca7415424978b7b95493fed283150
            locked(run).record_maps(batch, held, compiled.on_build);
        }
        return Attempt::Over(Done::Merged);
    };
    // The batch stopped: nothing of it is merged and the copy is left in place for a person
    // to read. A second rejection leaves its work beside the root for the next compile.
    // @lfy def/cli/main.lfy:main#main:main:4b34b56a504ad5f2803d8a2f2485ed28fe60ae7ebee92b236bd9c7c42e93773e
    // @lfy def/cli/main.lfy:main#main:main:fb97a602debdf6ca506dd68f399ab346accef55afb53336cff20a92a377060a1
    let Some(held) = compiled.held else {
        // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
        if compiled.keeps {
            write_kept(run, batch, root, mirror, &compiled.problems);
        }
        return Attempt::Over(Done::Stopped);
    };
    let waited: Vec<usize> = took.keys().copied().collect();
    // Nothing of the batch is merged into the root while a batch whose accepted changes its
    // copy took has not had its own files merged into it.
    // @lfy def/cli/main.lfy:main#main:main:f5720ad6fd831e736d050c0b20f67ced9fd4c44fe185806606e36c8d267767fc
    if round.wait_for(&waited) {
        // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
        locked(run).stop_running(&batch.identifier);
        let mut state = locked(run);
        state.stop(batch);
        state.worsen(ExitCode::Problems);
        return Attempt::Over(Done::Stopped);
    }
    // What the batches it took changes from actually wrote into the root: where a merge of
    // theirs mixed in other work, the copy takes those files and is built and tested again.
    // @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
    let mut differs: Changes = BTreeMap::new();
    // The batches whose files differ, in plan order, so the retrying line can name them.
    // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
    let mut differing: Vec<&str> = Vec::new();
    for (&waited, changes) in &took {
        let mut any = false;
        for (path, change) in changes {
            // @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
            let now = fs::read(root.join(path)).ok();
            if now.as_deref() == change.text.as_deref() {
                continue;
            }
            any = true;
            // The base of the merge into the copy is what the copy took for that file.
            // @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
            differs.insert(path.clone(), Change { base: change.text.clone(), text: now, mode: change.mode });
        }
        // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
        if any {
            differing.push(&round.planned[waited].batch.identifier);
        }
    }
    // @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
    if !differs.is_empty()
        && let Err(why) = remerge(run, batch, mirror, &differs, &runs)
    {
        // Nothing of the batch is merged, a retrying line names the batch whose files
        // differ, and the batch runs again from a new copy taking its changes as kept work.
        // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
        let message = format!("the files of {} differ: {why}", differing.join(", "));
        // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
        locked(run).reporter.report(Step::Retrying, Some(&batch.identifier), None, &message);
        // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
        let again = kept_of(run, batch, mirror, &Vec::new());
        mirror.remove();
        return Attempt::Again(again);
    }
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    match mirror.merge(root) {
        // Every file merged: the source maps are recorded in the root and the copy is
        // removed.
        // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
        Ok(_) => {
            // @lfy def/cli/main.lfy:main#main:main:e7d188f1e2558f6ab540ea91a682e317f0402c6c88a2e8b1868e626094e26a25
            locked(run).record_maps(batch, &held, compiled.on_build);
            // @lfy def/cli/main.lfy:main#main:main:d4df2511718d78630ac13bf572eebf1307e182b63f7ed82d414c8f8535fc1278
            mirror.remove();
            Attempt::Over(Done::Merged)
        }
        // A file could not be merged without a conflict: nothing of the batch is merged, a
        // retrying line names the file, and the batch runs again from a new copy once no
        // other batch is running. This is not a rejection, so no count and no code change.
        // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
        Err(path) => {
            // Nothing of the batch is merged, and this is not a rejection.
            // @lfy def/cli/main.lfy:main#main:main:c004e1659461201158916168dc6b9d94ea0c6c129329cb862b1796902b1d5fdb
            // @lfy def/cli/main.lfy:main#main:main:a247581eedaee59d3678954f47b4cd5e987c056104b4042f1edc50229d71c21a
            // A retrying progress line names the file.
            // @lfy def/cli/main.lfy:main#main:main:f4aa516e10c668a136c54381377ba1ca01d74e8e80dfb1d323ad816c23263d22
            let message = format!("{path} changed under the batch and could not be merged");
            locked(run).reporter.report(Step::Retrying, Some(&batch.identifier), None, &message);
            // The copy goes, since the batch runs again from a new one; what its units were
            // counted as stands, so the run anew counts them once rather than twice.
            // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
            mirror.remove();
            Attempt::Over(Done::Conflicted)
        }
    }
}

/// The batch's directory made: the root itself, or a copy of it seeded with the accepted
/// changes of each batch it depends on whose files are not in the root yet.
///
/// Once the root's files are copied, those changes are merged in, in plan order, as a batch's
/// files are merged into the root; what the copy then holds is kept as the batch's base. A
/// change that cannot be merged without a conflict has the copy removed and the batch waiting
/// for that batch to merge before it tries again. Nothing when a batch it waits on stopped.
// @lfy def/cli/main.lfy:main#main:main:f3bb66e88d43cf4578514a8d298ea474abb5cefc6ca529992953e31d189978e8
// @lfy def/cli/main.lfy:main#main:main:49481a50bf94e617c823f0536f8d44cdb0ca79898e0c7d84e0bd7ac9070d5960
// @lfy def/cli/main.lfy:main#main:main:1a2d8036f4189865cc6aa7e17b9a517f77ac124aa2c4c40497f8faf1b84c4f17
pub(crate) fn make_copy(
    run: &Mutex<Run>,
    batch: &Batch,
    root: &Path,
    round: &Round,
) -> Option<(Where, BTreeMap<usize, Changes>)> {
    loop {
        // @lfy def/cli/main.lfy:main#main:main:f3bb66e88d43cf4578514a8d298ea474abb5cefc6ca529992953e31d189978e8
        let to_take = round.to_take();
        // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
        let mut where_ = Where::of(root, &batch.identifier, round.copies);
        let Where::Copy(mirror) = &mut where_ else {
            return Some((where_, BTreeMap::new()));
        };
        let mut took: BTreeMap<usize, Changes> = BTreeMap::new();
        let mut conflicted: Option<usize> = None;
        // The changes are merged in, in plan order, as a batch's files are merged into the
        // root.
        // @lfy def/cli/main.lfy:main#main:main:f3bb66e88d43cf4578514a8d298ea474abb5cefc6ca529992953e31d189978e8
        for (waited, changes) in to_take {
            // @lfy def/cli/main.lfy:main#main:main:f3bb66e88d43cf4578514a8d298ea474abb5cefc6ca529992953e31d189978e8
            match merge_changes(&changes, &mirror.directory, &mirror.scratch()) {
                Ok(_) => {
                    took.insert(waited, changes);
                }
                // @lfy def/cli/main.lfy:main#main:main:49481a50bf94e617c823f0536f8d44cdb0ca79898e0c7d84e0bd7ac9070d5960
                Err(path) => {
                    let message = format!(
                        "{path} of {} could not be taken into the copy",
                        round.planned[waited].batch.identifier
                    );
                    locked(run).reporter.append(&message);
                    conflicted = Some(waited);
                    break;
                }
            }
        }
        // The changes of a batch could not be merged into the copy: the copy is removed and
        // the batch starts only once that batch has its files merged into the root.
        // @lfy def/cli/main.lfy:main#main:main:49481a50bf94e617c823f0536f8d44cdb0ca79898e0c7d84e0bd7ac9070d5960
        if let Some(waited) = conflicted {
            // @lfy def/cli/main.lfy:main#main:main:49481a50bf94e617c823f0536f8d44cdb0ca79898e0c7d84e0bd7ac9070d5960
            mirror.remove();
            round.wait_for_one(waited);
            // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
            if round.held().states[waited] == State::Stopped {
                return None;
            }
            continue;
        }
        // What each file of the copy holds once the changes it takes are merged in, and
        // before any kept work is taken, is kept as the batch's base.
        // @lfy def/cli/main.lfy:main#main:main:1a2d8036f4189865cc6aa7e17b9a517f77ac124aa2c4c40497f8faf1b84c4f17
        mirror.rebase();
        return Some((where_, took));
    }
}

/// Every batch of a round run: a batch starts once every batch holding a dependency of one of
/// its units is done, at most --jobs run at once, and among those that can start they start in
/// plan order. The batches that never started are the ones that did not complete.
// @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
pub(crate) fn run_batches(
    run: &Mutex<Run>,
    planned: &[Planned],
    root: &Path,
    compiler: Option<&str>,
    accept_only: bool,
    jobs: usize,
) {
    let schedule = Mutex::new(Schedule::of(planned));
    let finished = Condvar::new();
    // A batch runs in a copy of the root when more than one may run at once and the root is a
    // git repository; it runs in the root itself otherwise.
    // @lfy def/cli/main.lfy:main#main:main:cab5d4506292d57bf47d916ae73f8496c6abca998665c7cf57c137c631bd51e0
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    let copies = jobs > 1 && git_directory(root).is_some();
    std::thread::scope(|scope| {
        let mut running = Vec::new();
        loop {
            // The lock on the compile is taken and given back before the schedule's, so
            // that no thread ever holds both.
            // @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
            let halted = locked(run).halted;
            let mut schedules = schedule.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
            match schedules.next(planned, jobs, halted, copies) {
                // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
                Next::Start(at) => {
                    drop(schedules);
                    let batch = planned[at].batch.clone();
                    // A stopped batch stops only the batches that depend on one of its
                    // units; those never run, and count as not completed.
                    // @lfy def/cli/main.lfy:main#main:main:fef239376f4395cd11e23eb728a352b07322301b631d9069086add1723844c11
                    if locked(run).depends_on_stopped(&batch) {
                        let mut state = locked(run);
                        // @lfy def/cli/main.lfy:main#main:main:df6beb5b55efabb4bf57333ff81f1f65c615c2988fc6090856921440a10c3639
                        state.incomplete.push(batch.identifier.clone());
                        state.stopped.extend(batch.units.iter().copied());
                        drop(state);
                        schedule
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .finish(at, &Done::Stopped);
                        finished.notify_all();
                        continue;
                    }
                    let schedule = &schedule;
                    let finished = &finished;
                    running.push(scope.spawn(move || {
                        // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
                        let round = Round { schedule, finished, planned, at, copies };
                        let done = match (accept_only, compiler) {
                            // @lfy def/cli/main.lfy:main#main:main:e6df0c7548d8f79abfca3e578a974cfbbf2949927b4851d9860223b1315c6c27
                            (true, _) => {
                                Run::accept_on_disk(run, &batch, root);
                                Done::Merged
                            }
                            // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
                            (false, Some(command)) => run_batch(run, &batch, command, root, &round),
                            // @lfy def/cli/main.lfy:main#main:main:de437a586cddefeefabed78d855e65916788f25c33c6c0d9b7286be02d199ce5
                            // @lfy def/cli/main.lfy:main#main:main:3a07f6a4f45336a9a4e1b203912309522b7f93919e3caacfb488b3638af551bd
                            (false, None) => {
                                locked(run).write_request(&batch);
                                Done::Merged
                            }
                        };
                        schedule.lock().unwrap_or_else(std::sync::PoisonError::into_inner).finish(at, &done);
                        finished.notify_all();
                    }));
                }
                // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
                Next::Wait => {
                    drop(finished.wait(schedules));
                }
                // @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
                Next::Over => {
                    let unstarted = schedules.unstarted();
                    drop(schedules);
                    let mut state = locked(run);
                    for at in unstarted {
                        // @lfy def/cli/main.lfy:main#main:main:df6beb5b55efabb4bf57333ff81f1f65c615c2988fc6090856921440a10c3639
                        state.incomplete.push(planned[at].batch.identifier.clone());
                        state.stopped.extend(planned[at].batch.units.iter().copied());
                    }
                    break;
                }
            }
        }
        // The batches already running finish as they would, and the global review waits for
        // every one of them, so it reads a root every batch has been merged into.
        // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
        // @lfy def/cli/main.lfy:main#main:main:b560d79fb66587371f4a98bc89eea6db2f1123b1c6455f3310c1a9827b77b43e
        for handle in running {
            let _ = handle.join();
        }
    });
}

/// The work a rejected run of this batch kept, when `elfie-compile/cache/kept/<batch>.json`
/// under the root names the batch's target and the stems of its units in batch order and
/// gives each unit the hash and the requirements that are its own now; nothing otherwise.
// @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
// @lfy def/cli/main.lfy:main#main:main:e12cb0bab33ffea8be1bf737d72e297e3f70fc95f34b089abc10cc9071b1e079
pub(crate) fn kept_work(run: &Mutex<Run>, batch: &Batch, root: &Path) -> Option<Kept> {
    let path = kept_file(root, &batch.identifier);
    // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
    let text = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let held = Kept::of_json(&value)?;
    // @lfy def/cli/main.lfy:main#main:main:e12cb0bab33ffea8be1bf737d72e297e3f70fc95f34b089abc10cc9071b1e079
    let asked = locked(run).asked_kept(batch);
    // @lfy def/cli/main.lfy:main#main:main:e12cb0bab33ffea8be1bf737d72e297e3f70fc95f34b089abc10cc9071b1e079
    held.is_for(&asked).then_some(held)
}

/// The kept work taken into a batch's copy, after the base was kept: each file it holds is
/// merged into the copy as the three-way merge of what the copy holds for it and its kept
/// text against its kept base, and a file whose merge has a conflict is left as the copy
/// holds it, which is neither a rejection nor a reason to run the batch again.
///
/// One resuming progress line counts the files taken and the files left. Nothing is taken
/// when the batch runs in the root, because a rejected batch's outputs are left on disk
/// there.
// @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
// @lfy def/cli/main.lfy:main#main:main:8444ea8a2d9ff6b491b88ffdab12f7f13f9596b9dfd77f20d2a51a121c7cad69
pub(crate) fn take_kept(run: &Mutex<Run>, batch: &Batch, where_: &Where, kept: Option<&Kept>) -> Option<Kept> {
    let held = kept?;
    // @lfy def/cli/main.lfy:main#main:main:8444ea8a2d9ff6b491b88ffdab12f7f13f9596b9dfd77f20d2a51a121c7cad69
    let Where::Copy(mirror) = where_ else {
        return None;
    };
    let scratch = mirror.scratch();
    let mut taken = 0;
    let mut left = 0;
    for file in &held.files {
        let at = mirror.directory.join(&file.path);
        let now = fs::read(&at).ok();
        // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
        let merged = match (&file.base, &file.text, &now) {
            // The copy holds what the kept work was based on: the kept text stands.
            // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
            (base, text, now) if base.as_deref().map(str::as_bytes) == now.as_deref() => text.clone(),
            // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
            (Some(base), Some(text), Some(now)) => {
                merged(now, base.as_bytes(), text.as_bytes(), &scratch)
                    .map(|held| String::from_utf8_lossy(&held).into_owned())
            }
            // One of the three missing is a file added or removed on both sides, which no
            // merge settles.
            // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
            _ => None,
        };
        // A kept file whose merge has a conflict is left as the copy holds it.
        // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
        let Some(text) = merged else {
            left += 1;
            continue;
        };
        // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
        if let Some(parent) = at.parent() {
            let _ = fs::create_dir_all(parent);
        }
        // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
        let written = if text.is_empty() && file.text.is_none() {
            fs::remove_file(&at).is_ok()
        } else {
            fs::write(&at, &text).is_ok()
        };
        if written {
            taken += 1;
        } else {
            left += 1;
        }
    }
    // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
    let message = format!("{taken} files taken, {left} left");
    locked(run).reporter.report(Step::Resuming, Some(&batch.identifier), None, &message);
    Some(Kept {
        target: held.target.clone(),
        units: Vec::new(),
        // @lfy def/cli/main.lfy:main#main:main:42f4bccc722123748448fab03bfe2e9f1e2ec7530f09efd662aa8ce9f1cb26a2
        problems: held.problems.clone(),
        files: Vec::new(),
    })
}

/// The kept work of a batch: its target, its units, the problems of the rejection that
/// stopped it, and each file of its copy that would be merged into the root were it done, in
/// path order.
// @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
pub(crate) fn kept_of(run: &Mutex<Run>, batch: &Batch, mirror: &Mirror, problems: &[String]) -> Kept {
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    let mut kept = locked(run).asked_kept(batch);
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    kept.problems = problems.to_vec();
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    kept.files = mirror
        .changes()
        .into_iter()
        .map(|(path, change)| KeptFile {
            path,
            // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
            base: change.base.map(|held| String::from_utf8_lossy(&held).into_owned()),
            // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
            text: change.text.map(|held| String::from_utf8_lossy(&held).into_owned()),
        })
        .collect();
    kept
}

/// The kept work of a batch written to `elfie-compile/cache/kept/<batch>.json` under the
/// root, replacing what that file held, before its copy is left in place.
// @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
pub(crate) fn write_kept(run: &Mutex<Run>, batch: &Batch, root: &Path, mirror: &Mirror, problems: &[String]) {
    // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
    let kept = kept_of(run, batch, mirror, problems);
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    let text = format!("{}\n", serde_json::to_string_pretty(&kept.to_json()).unwrap_or_default());
    let path = kept_file(root, &batch.identifier);
    // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
    let written = path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&path, &text));
    if let Err(error) = written {
        let mut state = locked(run);
        complain(state.style.err, Some(&path.display().to_string()), &error.to_string());
        state.worsen(ExitCode::Failure);
    }
}

/// The files another batch's merge wrote into the root taken into this batch's copy, its base
/// set to them in place of what it took, and the target's build and test commands run there,
/// as they run for a batch whose units all have the reason dependency.
///
/// Nothing when they cannot be merged into the copy without a conflict, when either command
/// fails, or when the target has no line for build or for test: the reason the batch runs
/// again instead.
// @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
// @lfy def/cli/main.lfy:main#main:main:b4ffc629cbf563b3e2f3d8f96680345c330cce56013ce20eb8fb1217658eac0d
pub(crate) fn remerge(
    run: &Mutex<Run>,
    batch: &Batch,
    mirror: &mut Mirror,
    differs: &Changes,
    runs: &Runs,
) -> Result<(), String> {
    // @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
    if let Err(path) = merge_changes(differs, &mirror.directory, &mirror.scratch()) {
        // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
        return Err(format!("{path} reached the root with other work and could not be taken"));
    }
    // The base takes the files that batch's merge wrote into the root in place of those it
    // took.
    // @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
    for (path, change) in differs {
        match &change.text {
            Some(text) => {
                mirror.base.insert(path.clone(), text.clone());
            }
            None => {
                mirror.base.remove(path);
            }
        }
    }
    // The target has no line for build or for test: the batch runs again rather than being
    // merged on nothing.
    // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
    if !locked(run).builds_and_tests(batch) {
        // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
        return Err("the target has no build and test to check the merge with".to_string());
    }
    // @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
    // @lfy def/cli/main.lfy:main#main:main:b4ffc629cbf563b3e2f3d8f96680345c330cce56013ce20eb8fb1217658eac0d
    let directory = mirror.directory.clone();
    Run::build_and_test(run, batch, &directory, runs)
}
