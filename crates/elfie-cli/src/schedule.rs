// @lfy def/cli/main.lfy:compile
//! The units of a compile run at once, as many as --jobs lets: which unit starts next, the copy
//! each one runs in, what one unit's run comes to from its first run to its outcome, how a unit
//! done in its copy merges into its change set, what stops the compile or skips a unit, and
//! what the foreign edits a merged unit made do to the units after it.

use super::*;
use crate::compiling::{append_problems, keep_kept_units};
use crate::copies::{
    UnitCopy, changed_in, held_at, is_output, make_unit_copy, merges_of, own_contents, put_back, remove_directory,
    start_of, unit_copy_of, write_kept, write_merges,
};
use crate::processes::{Runs, line_for, run_target_command, word_of_operation};
use crate::running::{Run, Stop, files_in, own_marker, own_region_text};

/// What a unit's first run is: the compiler, or what may stand in for it.
// @lfy def/cli/main.lfy:compile#compile:compile:d43663ac22c59c11af52567c184b43ae8ce9a8ccec6837a4f398f5a82a41c8e5
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Stage {
    /// The compiler runs on the unit, as on any unit.
    // @lfy def/cli/main.lfy:compile#compile:compile:d43663ac22c59c11af52567c184b43ae8ce9a8ccec6837a4f398f5a82a41c8e5
    Compiler,
    /// The unit is reworded only, so moving the markers of its reworded criteria and tests is
    /// the whole of the work and its outputs are checked without the compiler.
    // @lfy def/cli/main.lfy:compile#compile:compile:80966c9bedadf4c34910a903c33a537e23616ffb635289db0d2d5adc02ef1aa7
    Reworded,
    /// The unit has the reason dependency, so the unit gate stands in for the compiler.
    // @lfy def/cli/main.lfy:compile#compile:compile:f82d76ff8e23e283093211b99bd7fa4f7edfa0a9f9151cf64b7864379ceec690
    Dependency,
    /// The unit was touched, and every foreign edit made in it is a fix, so its outputs are
    /// checked as they are without the compiler.
    // @lfy def/cli/main.lfy:compile#compile:compile:b44cd9235b564a50b5d58b80e928bf32262b3d5b69b8aad8ef60c1bdcfa59259
    Fixes,
}

/// What working one unit in its copy came to.
// @lfy def/cli/main.lfy:compile#compile:compile:acabbfb20c858bf034438b84b716e4a6a9d443d7c7d563d0e3fae30099d78448
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ended {
    /// It is done in its copy, to merge in this state, with the foreign edits of its last
    /// accepted verdict.
    // @lfy def/cli/main.lfy:compile#compile:compile:acabbfb20c858bf034438b84b716e4a6a9d443d7c7d563d0e3fae30099d78448
    Done(UnitState, Vec<ForeignEdit>),
    /// It stopped without being accepted.
    Stopped(Stop),
}

/// What a unit's run of the compiler and of the verifier has come to so far in its copy,
/// which decides whether it runs once more or stops.
// @lfy def/cli/main.lfy:compile#compile:compile:1672da5d763ee2c0c8a1a0e012ba1eb3a94200faedfbafa2b5a74254c696bfb6
// @lfy def/cli/main.lfy:compile#compile:compile:91514d592807a6eae69b04199835ae037c731478013def307e41bb393d1f98c3
#[derive(Default)]
pub(crate) struct Tries {
    /// How many times the compiler ran on the unit; a skipped first run counts as none.
    // @lfy def/cli/main.lfy:compile#compile:compile:1672da5d763ee2c0c8a1a0e012ba1eb3a94200faedfbafa2b5a74254c696bfb6
    pub(crate) compiled: usize,
    /// Whether an outcome of the compiler was rejected, or the unit gate failed after it.
    // @lfy def/cli/main.lfy:compile#compile:compile:4f06b5aa3049fb148a394aa3261e0dc49c2e9cb564b4604bea58741f2a16204f
    pub(crate) rejected: bool,
    /// Whether the verifier's report held a violated review or an unplaced violation.
    // @lfy def/cli/main.lfy:compile#compile:compile:989f6f309c8ec4cc4f9e9682676ed8016bcfaac8a5f6ccf14d33c1bc0207a0ec
    pub(crate) violated: bool,
    /// Whether a run of the compiler failed or could not be started.
    // @lfy def/cli/main.lfy:compile#compile:compile:c836eb712f2d9528c78321a4094d806dd4bf34577eaf2bab287bf88d3e18d9f5
    pub(crate) failed: bool,
    /// Whether the next run of the compiler goes on from the session of its run before it.
    // @lfy def/cli/main.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
    pub(crate) resume_compiler: bool,
    /// Whether the next run of the verifier goes on from the session of its run before it.
    // @lfy def/cli/main.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
    pub(crate) resume_verifier: bool,
}

/// What one unit's run in its copy gives back to its compile once it has returned.
// @lfy def/cli/main.lfy:compile#compile:compile:64612b8370c169cfba1b380bcbd8e3e7396cfd3a0ff4f100a5d0f40becf6bd69
pub(crate) struct Returned {
    pub(crate) unit: usize,
    pub(crate) ended: Ended,
    /// The worst code the run met.
    pub(crate) code: ExitCode,
    /// Whether the run stopped the compile, as an agent server that could not be hosted does.
    pub(crate) halted: bool,
}

/// Where the units of a compile's change sets stand while they run at once.
// @lfy def/cli/main.lfy:compile#compile:compile:8cf4943ef6637dd81797621b8abc048edcc7781a4f19546fbe99863339043826
#[derive(Default)]
pub(crate) struct Schedule {
    /// The units of the compile's first plan, each with the units of its [`Unit::waits_for`]
    /// as that plan gives them, by their places in the plan.
    // @lfy def/cli/main.lfy:compile#compile:compile:8cf4943ef6637dd81797621b8abc048edcc7781a4f19546fbe99863339043826
    pub(crate) first: BTreeMap<usize, Vec<usize>>,
    /// Every unit that merged in the compile, the units a resumed change set keeps among
    /// them.
    // @lfy def/cli/main.lfy:compile#compile:compile:42d392d488315c356a8aec4d1df3e5316914fd7797f15f57772a074b0e121165
    pub(crate) merged: BTreeSet<usize>,
    /// The units that merged since the start of their change set was made, whose merges a
    /// copy made from that start may be given.
    // @lfy def/cli/main.lfy:compile#compile:compile:3de63e91d36a4914ecba5efb6db7e4b1c99793fe3707ad3fcd9f39dcfb2252af
    pub(crate) since_start: BTreeSet<usize>,
    /// The stems of the units done in their change set when its start was made, whose merges
    /// every copy holds.
    // @lfy def/cli/main.lfy:compile#compile:compile:2125687777aae924ecaa844b9ab21cf364a6cb1234b7ce57b7f6c3c0550d179e
    pub(crate) at_start: BTreeSet<String>,
    /// The units whose copy is made as their change set's copy stands: those planned during
    /// the compile, and those whose merge was undone.
    // @lfy def/cli/main.lfy:compile#compile:compile:9fb81fa1507e767993a4d11085badfdfe00d34fe56bfd3c332d91cc5d3215da5
    // @lfy def/cli/main.lfy:compile#compile:compile:4f3ee5302864ac43e74b609edd4d32ee20f6d99060ed1e0babcc498c956098ff
    pub(crate) again: BTreeSet<usize>,
    /// The units whose last merge was undone.
    // @lfy def/cli/main.lfy:compile#compile:compile:ee6f110a6e8a80fe7f058125db6f1cd190ee42098edb6f0deaed19ccf0a70a19
    pub(crate) undone: BTreeSet<usize>,
    /// The copy of each unit that started, by its place in the plan, until it is removed.
    // @lfy def/cli/main.lfy:compile#compile:compile:649dfaca80ede5a5de3cfecd48f1de664e250e9bc65773266822f3f63ae34c71
    pub(crate) copies: BTreeMap<usize, UnitCopy>,
    /// The units running.
    // @lfy def/cli/main.lfy:compile#compile:compile:64612b8370c169cfba1b380bcbd8e3e7396cfd3a0ff4f100a5d0f40becf6bd69
    pub(crate) running: BTreeSet<usize>,
    /// The units done in their copy that wait to merge, each with the state it merges in and
    /// the foreign edits of its last accepted verdict.
    // @lfy def/cli/main.lfy:compile#compile:compile:acabbfb20c858bf034438b84b716e4a6a9d443d7c7d563d0e3fae30099d78448
    pub(crate) done: BTreeMap<usize, (UnitState, Vec<ForeignEdit>)>,
}

/// What one merge into a change set's copy came to.
// @lfy def/cli/main.lfy:compile#compile:compile:a758d1de98247fd21db6f348f4b6ad304727df53a1f2655c9c1794be92ac7fc5
enum Merge {
    /// It merged, writing or removing these paths.
    Merged(Vec<String>),
    /// It was undone, for this reason.
    Undone(String),
}

impl Run {
    /// Every planned unit of every change set run and merged.
    ///
    /// At most as many units run at once, across every target, as --jobs says; a unit that
    /// is done in its copy and waits to merge does not count among them. When more units are
    /// ready than may run, the earliest in plan order starts first. A unit of the compile's
    /// first plan is ready once every unit it waits for has merged, been skipped, or stopped;
    /// any other unit once every planned unit before it in its change set has. Units merge in
    /// the order of their change set. A unit that depends, directly or through other units, on
    /// one that stopped is skipped with --continue, and without it the compile stops: no unit
    /// starts or merges after that, and every run still going is stopped and what it gives is
    /// ignored.
    // @lfy def/cli/main.lfy:compile#compile:compile:8cf4943ef6637dd81797621b8abc048edcc7781a4f19546fbe99863339043826
    // @lfy def/cli/main.lfy:compile#compile:compile:1fd9be2beb849bc4a65969dbedc075ab348b763ef108d6fbc41f4001bc8a86b9
    // @lfy def/cli/main.lfy:compile#compile:compile:64612b8370c169cfba1b380bcbd8e3e7396cfd3a0ff4f100a5d0f40becf6bd69
    // @lfy def/cli/main.lfy:compile#compile:compile:acabbfb20c858bf034438b84b716e4a6a9d443d7c7d563d0e3fae30099d78448
    // @lfy def/cli/main.lfy:compile#compile:compile:51b22c76c1410a187c0f287178a9ecb28048b54a983754ad873fa703eb7809a3
    pub(crate) fn run_units(&mut self, command: &str) {
        let (sender, receiver) = std::sync::mpsc::channel::<Returned>();
        loop {
            // The compile stopped: no unit starts or merges after that, and what a run still
            // going gives, once it was stopped, is ignored.
            // @lfy def/cli/main.lfy:compile#compile:compile:51b22c76c1410a187c0f287178a9ecb28048b54a983754ad873fa703eb7809a3
            // @lfy def/cli/main.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
            if self.halted {
                while !self.schedule.running.is_empty() {
                    let Ok(returned) = receiver.recv() else { break };
                    self.schedule.running.remove(&returned.unit);
                }
                self.remove_every_copy();
                return;
            }
            // Merging first, so a unit waiting on one that merges may start now.
            // @lfy def/cli/main.lfy:compile#compile:compile:acabbfb20c858bf034438b84b716e4a6a9d443d7c7d563d0e3fae30099d78448
            if self.merge_next() {
                continue;
            }
            self.start_ready(command, &sender);
            if self.halted {
                continue;
            }
            // Nothing runs and nothing can merge: every unit that could has.
            if self.schedule.running.is_empty() {
                self.remove_every_copy();
                return;
            }
            let Ok(returned) = receiver.recv() else { return };
            self.returned(returned);
        }
    }

    /// As many ready units started as may run at once, the earliest in plan order first.
    // @lfy def/cli/main.lfy:compile#compile:compile:1fd9be2beb849bc4a65969dbedc075ab348b763ef108d6fbc41f4001bc8a86b9
    // @lfy def/cli/main.lfy:compile#compile:compile:7ca8d55384b2971cfabab5ffdce367c3faadb58b56ff385b0a2860676eae5360
    pub(crate) fn start_ready(&mut self, command: &str, sender: &std::sync::mpsc::Sender<Returned>) {
        // @lfy def/cli/main.lfy:compile#compile:compile:1fd9be2beb849bc4a65969dbedc075ab348b763ef108d6fbc41f4001bc8a86b9
        while self.schedule.running.len() < self.jobs && !self.halted {
            // @lfy def/cli/main.lfy:compile#compile:compile:7ca8d55384b2971cfabab5ffdce367c3faadb58b56ff385b0a2860676eae5360
            let Some(unit) = self.next_ready() else { return };
            self.start_unit(unit, command, sender);
        }
    }

    /// The earliest unit in plan order that is ready to start; a unit that depends on one that
    /// stopped is skipped on the way.
    // @lfy def/cli/main.lfy:compile#compile:compile:7ca8d55384b2971cfabab5ffdce367c3faadb58b56ff385b0a2860676eae5360
    // @lfy def/cli/main.lfy:compile#compile:compile:983a29cf93655951616c5c562ff1c6178966fa0f7acb433740f541446615af08
    pub(crate) fn next_ready(&mut self) -> Option<usize> {
        for unit in self.planned_units() {
            if self.set_of(unit).is_none() || !self.waiting(unit) {
                continue;
            }
            // @lfy def/cli/main.lfy:compile#compile:compile:983a29cf93655951616c5c562ff1c6178966fa0f7acb433740f541446615af08
            if self.depends_on_stopped(unit) {
                self.skipped.insert(unit);
                continue;
            }
            if self.ready(unit) {
                return Some(unit);
            }
        }
        None
    }

    /// Whether a planned unit is still to start: neither accepted nor reviewed, neither
    /// running nor done in its copy, and neither stopped nor skipped.
    // @lfy def/cli/main.lfy:compile#compile:compile:8cf4943ef6637dd81797621b8abc048edcc7781a4f19546fbe99863339043826
    pub(crate) fn waiting(&self, unit: usize) -> bool {
        !self.state_of(unit).is_done()
            && !self.schedule.running.contains(&unit)
            && !self.schedule.done.contains_key(&unit)
            && !self.stopped.contains(&unit)
            && !self.skipped.contains(&unit)
    }

    /// Whether a unit depends, directly or through other units, on one that stopped or was
    /// skipped.
    // @lfy def/cli/main.lfy:compile#compile:compile:983a29cf93655951616c5c562ff1c6178966fa0f7acb433740f541446615af08
    pub(crate) fn depends_on_stopped(&self, unit: usize) -> bool {
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut next: Vec<usize> = self.plan.units[unit].dependencies.clone();
        while let Some(dependency) = next.pop() {
            if !seen.insert(dependency) {
                continue;
            }
            if self.stopped.contains(&dependency) || self.skipped.contains(&dependency) {
                return true;
            }
            next.extend(self.plan.units[dependency].dependencies.iter().copied());
        }
        false
    }

    /// Whether a unit is ready to start: a unit of the compile's first plan once every unit
    /// of its [`Unit::waits_for`], as that plan gives it, has merged, been skipped, or stopped,
    /// a unit a resumed change set keeps counting as merged; any other unit, planned during
    /// the compile or run again after an undone merge, once every planned unit before it in
    /// its change set has.
    // @lfy def/cli/main.lfy:compile#compile:compile:8cf4943ef6637dd81797621b8abc048edcc7781a4f19546fbe99863339043826
    // @lfy def/cli/main.lfy:compile#compile:compile:9fb81fa1507e767993a4d11085badfdfe00d34fe56bfd3c332d91cc5d3215da5
    // @lfy def/cli/main.lfy:compile#compile:compile:42d392d488315c356a8aec4d1df3e5316914fd7797f15f57772a074b0e121165
    pub(crate) fn ready(&self, unit: usize) -> bool {
        // @lfy def/cli/main.lfy:compile#compile:compile:8cf4943ef6637dd81797621b8abc048edcc7781a4f19546fbe99863339043826
        if let Some(waits) = self.schedule.first.get(&unit)
            && !self.schedule.again.contains(&unit)
        {
            return waits.iter().all(|&other| {
                self.schedule.merged.contains(&other)
                    || self.stopped.contains(&other)
                    || self.skipped.contains(&other)
                    || !self.in_change_set(other)
            });
        }
        // @lfy def/cli/main.lfy:compile#compile:compile:9fb81fa1507e767993a4d11085badfdfe00d34fe56bfd3c332d91cc5d3215da5
        self.settled_before(unit)
    }

    /// Whether a unit is one of its change set's units.
    pub(crate) fn in_change_set(&self, unit: usize) -> bool {
        let stem = &self.plan.units[unit].stem;
        self.set_of(unit).is_some_and(|set| set.units.iter().any(|held| &held.stem == stem))
    }

    /// Whether every planned unit before a unit in its change set has merged, been skipped,
    /// or stopped.
    // @lfy def/cli/main.lfy:compile#compile:compile:9fb81fa1507e767993a4d11085badfdfe00d34fe56bfd3c332d91cc5d3215da5
    // @lfy def/cli/main.lfy:compile#compile:compile:acabbfb20c858bf034438b84b716e4a6a9d443d7c7d563d0e3fae30099d78448
    pub(crate) fn settled_before(&self, unit: usize) -> bool {
        let Some(set) = self.set_of(unit) else { return true };
        let stem = &self.plan.units[unit].stem;
        for held in &set.units {
            if &held.stem == stem {
                return true;
            }
            let Some(other) = self.plan.units.iter().position(|planned| planned.stem == held.stem) else { continue };
            if !(held.state.is_done() || self.stopped.contains(&other) || self.skipped.contains(&other)) {
                return false;
            }
        }
        true
    }

    /// The units a unit waits for, directly or through the units of their
    /// [`Unit::waits_for`], as the compile's first plan gives them, in plan order.
    // @lfy def/cli/main.lfy:compile#compile:compile:3de63e91d36a4914ecba5efb6db7e4b1c99793fe3707ad3fcd9f39dcfb2252af
    pub(crate) fn waited_for(&self, unit: usize) -> Vec<usize> {
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut next: Vec<usize> = self.schedule.first.get(&unit).cloned().unwrap_or_default();
        while let Some(other) = next.pop() {
            if seen.insert(other) {
                next.extend(self.schedule.first.get(&other).cloned().unwrap_or_default());
            }
        }
        seen.into_iter().collect()
    }

    /// One unit started in a copy of its own: its previous source recovered before anything
    /// else is done for it, its copy made, one started line saying how many units run once it
    /// started, and its run begun apart from the compile, given the foreign edits recorded
    /// since it was last accepted that a unit its copy holds the merge of made.
    ///
    /// A unit of the compile's first plan has its copy made from its change set's start, with
    /// the merges of the units it waits for, directly or through theirs, that merged since that
    /// start was made, in plan order; any other unit's is made as its change set's copy stands,
    /// and holds the merge of every unit merged when it was made.
    // @lfy def/cli/main.lfy:compile#compile:compile:649dfaca80ede5a5de3cfecd48f1de664e250e9bc65773266822f3f63ae34c71
    // @lfy def/cli/main.lfy:compile#compile:compile:3de63e91d36a4914ecba5efb6db7e4b1c99793fe3707ad3fcd9f39dcfb2252af
    // @lfy def/cli/main.lfy:compile#compile:compile:4f3ee5302864ac43e74b609edd4d32ee20f6d99060ed1e0babcc498c956098ff
    // @lfy def/cli/main.lfy:compile#compile:compile:2125687777aae924ecaa844b9ab21cf364a6cb1234b7ce57b7f6c3c0550d179e
    // @lfy def/cli/main.lfy:compile#compile:compile:932f229a081364bb85c06954c07796f035ea0cd10a0285dc8a983a60bbe1fcc8
    // @lfy def/cli/main.lfy:compile#compile:compile:d9ff66213993aa0d26ba013faf314fca87796340ce0cfd4bbd4da57f20bbf6ce
    pub(crate) fn start_unit(&mut self, unit: usize, command: &str, sender: &std::sync::mpsc::Sender<Returned>) {
        // @lfy def/cli/main.lfy:compile#compile:compile:0e1f26aad48b4a2e79096c71738d1754b0ac48ecec6dc143f7bb9c5a1e48987d
        self.recover(unit);
        let root = self.root();
        let target = self.target_of_unit(unit).clone();
        let stem = self.stem(unit);
        let Some(set) = self.set_of(unit).cloned() else { return };
        // @lfy def/cli/main.lfy:compile#compile:compile:649dfaca80ede5a5de3cfecd48f1de664e250e9bc65773266822f3f63ae34c71
        let at = unit_copy_of(&root, &target.identifier, &stem);
        let from_start = self.schedule.first.contains_key(&unit) && !self.schedule.again.contains(&unit);
        let mut holds: BTreeSet<String> = self.schedule.at_start.clone();
        let made = if from_start {
            // @lfy def/cli/main.lfy:compile#compile:compile:3de63e91d36a4914ecba5efb6db7e4b1c99793fe3707ad3fcd9f39dcfb2252af
            let merges: Vec<usize> =
                self.waited_for(unit).into_iter().filter(|other| self.schedule.since_start.contains(other)).collect();
            make_unit_copy(&root, &target, &at, &start_of(&root, &target.identifier)).and_then(|()| {
                for &other in &merges {
                    let name = self.stem(other);
                    let paths = set.units.iter().find(|held| held.stem == name).map(|held| held.merged.clone()).unwrap_or_default();
                    let kept = ChangeSet::merged_directory(&root, &target.identifier, &request_name(&name));
                    write_kept(&kept, &paths, &at)?;
                    // @lfy def/cli/main.lfy:compile#compile:compile:2125687777aae924ecaa844b9ab21cf364a6cb1234b7ce57b7f6c3c0550d179e
                    holds.insert(name);
                }
                Ok(())
            })
        } else {
            // @lfy def/cli/main.lfy:compile#compile:compile:4f3ee5302864ac43e74b609edd4d32ee20f6d99060ed1e0babcc498c956098ff
            // @lfy def/cli/main.lfy:compile#compile:compile:2125687777aae924ecaa844b9ab21cf364a6cb1234b7ce57b7f6c3c0550d179e
            holds.extend(self.schedule.since_start.iter().map(|&other| self.stem(other)));
            make_unit_copy(&root, &target, &at, Path::new(&set.directory))
        };
        // A copy that cannot be made stops the compile, and the code is failure.
        if let Err(error) = made {
            complain(self.style.err, Some(&at.display().to_string()), &error.to_string());
            self.worsen(ExitCode::Failure);
            self.halted = true;
            self.running.stop();
            return;
        }
        let copy = UnitCopy { made: own_contents(&at, &target), directory: at.clone(), holds };
        // The foreign edits recorded since the unit was last accepted, made by a unit its copy
        // holds the merge of, in their order.
        // @lfy def/cli/main.lfy:compile#compile:compile:d9ff66213993aa0d26ba013faf314fca87796340ce0cfd4bbd4da57f20bbf6ce
        let edits: Vec<ForeignEdit> =
            set.edits_since(&stem).into_iter().filter(|edit| copy.holds.contains(&edit.by)).cloned().collect();
        self.schedule.copies.insert(unit, copy);
        self.schedule.running.insert(unit);
        // @lfy def/cli/main.lfy:compile#compile:compile:932f229a081364bb85c06954c07796f035ea0cd10a0285dc8a983a60bbe1fcc8
        let message = format!("{} of {} running", self.schedule.running.len(), self.jobs);
        self.reporter.report(Step::Started, Some(&stem), &message);
        let mut fork = self.fork(unit, edits);
        let sender = sender.clone();
        let command = command.to_string();
        std::thread::spawn(move || {
            // A run that panics is a run that failed, so the compile is not left waiting on it.
            let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fork.work_unit(unit, &command, &at)))
                .unwrap_or(Ended::Stopped(Stop::Failed));
            let _ = sender.send(Returned { unit, ended, code: fork.code, halted: fork.halted });
        });
    }

    /// What a unit's run gave back taken in: it is done in its copy and waits to merge, or it
    /// stopped.
    // @lfy def/cli/main.lfy:compile#compile:compile:64612b8370c169cfba1b380bcbd8e3e7396cfd3a0ff4f100a5d0f40becf6bd69
    pub(crate) fn returned(&mut self, returned: Returned) {
        self.schedule.running.remove(&returned.unit);
        self.worsen(returned.code);
        // An agent server that could not be hosted stopped the compile.
        // @lfy def/cli/main.lfy:compile#compile:compile:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
        if returned.halted {
            self.halted = true;
            self.running.stop();
            return;
        }
        match returned.ended {
            // @lfy def/cli/main.lfy:compile#compile:compile:acabbfb20c858bf034438b84b716e4a6a9d443d7c7d563d0e3fae30099d78448
            Ended::Done(state, edits) => {
                self.schedule.done.insert(returned.unit, (state, edits));
            }
            Ended::Stopped(stop) => self.stopped_unit(returned.unit, stop),
        }
    }

    /// The first unit of a change set, in the order of the targets, that is done in its copy
    /// and whose every planned unit before it in its change set has merged, been skipped, or
    /// stopped, merged; or skipped, when it depends on a unit that stopped. Whether one was.
    // @lfy def/cli/main.lfy:compile#compile:compile:acabbfb20c858bf034438b84b716e4a6a9d443d7c7d563d0e3fae30099d78448
    // @lfy def/cli/main.lfy:compile#compile:compile:983a29cf93655951616c5c562ff1c6178966fa0f7acb433740f541446615af08
    pub(crate) fn merge_next(&mut self) -> bool {
        let waiting: Vec<usize> = self.schedule.done.keys().copied().collect();
        let mut ordered: Vec<(usize, usize, usize)> = Vec::new();
        for unit in waiting {
            let Some(set) = self.set_of(unit) else { continue };
            let stem = &self.plan.units[unit].stem;
            let place = set.units.iter().position(|held| &held.stem == stem).unwrap_or(usize::MAX);
            ordered.push((self.plan.units[unit].target, place, unit));
        }
        ordered.sort_unstable();
        for (_, _, unit) in ordered {
            if !self.settled_before(unit) {
                continue;
            }
            // A unit that depends on one that stopped is skipped, and nothing of it merges.
            // @lfy def/cli/main.lfy:compile#compile:compile:983a29cf93655951616c5c562ff1c6178966fa0f7acb433740f541446615af08
            if self.depends_on_stopped(unit) {
                self.schedule.done.remove(&unit);
                self.skipped.insert(unit);
                self.remove_copy(unit);
                return true;
            }
            let Some((state, edits)) = self.schedule.done.remove(&unit) else { continue };
            self.merge_unit(unit, state, edits);
            return true;
        }
        false
    }

    /// A unit done in its copy merged into its change set's copy, and accepted there; or its
    /// merge undone, when a region of its own or one it made a foreign edit in was changed by
    /// another unit since its copy was made, when a file it writes does not merge without a
    /// conflict, or when the target's build or its test with stubs allowed fails after it.
    ///
    /// Each file under the target's output directory that its copy holds otherwise than when
    /// the copy was made, or that was added or removed since, is written into, or removed
    /// from, the change set's copy: as the copy holds it when the change set's copy still holds
    /// what the unit's copy held when it was made, and as the three-way merge of both against
    /// that otherwise. Its map file and its review record file are written as its copy holds
    /// them and never merged; then the maps of the unit and of every unit with a marker of its
    /// own in a file the merge wrote are re-derived in the change set's copy and recorded
    /// there. When the merge wrote or removed a file under the output directory, the target's
    /// build and then its test with stubs allowed run in the change set's copy, as the unit
    /// gate runs them, each only once the one before it succeeded; otherwise no command runs.
    // @lfy def/cli/main.lfy:compile#compile:compile:89c7d76425815427ea012671f3b4f7dddc0031ff70cc737c12eebf4300797c95
    // @lfy def/cli/main.lfy:compile#compile:compile:e1494d50bc4ca15b22408682181b99b6a82a81da36ad36697a5ce42702ba41d9
    // @lfy def/cli/main.lfy:compile#compile:compile:dd602ad17aa5add15d7e0b72130ebaa6955e268ab8b31ac9d2c28c819c36a028
    // @lfy def/cli/main.lfy:compile#compile:compile:e97aeb05e9fe84cdc671d0ff5fc5ad164bbca228900888f23cc6a7beedcdb985
    // @lfy def/cli/main.lfy:compile#compile:compile:440bee8e71d6f1c2ae85585e6d67504b3322b68096a7a0167d6acade8f10fde8
    // @lfy def/cli/main.lfy:compile#compile:compile:55e0af72225acb1c6b134a3811478ff630be96bdc40d1f40970151a84cb1eada
    // @lfy def/cli/main.lfy:compile#compile:compile:77eb0c2c3b5559ce6700a2a65f66921a86b34f2759727cb081ba2ffcc3450fa0
    pub(crate) fn merge_unit(&mut self, unit: usize, state: UnitState, edits: Vec<ForeignEdit>) {
        match self.merged_into_set(unit, &edits) {
            // @lfy def/cli/main.lfy:compile#compile:compile:55e0af72225acb1c6b134a3811478ff630be96bdc40d1f40970151a84cb1eada
            Merge::Merged(paths) => {
                self.accept_merged(unit, state, edits, paths);
                // @lfy def/cli/main.lfy:compile#compile:compile:38e29db3efde6fef8e4f2e633ff44b5dee097d14f8bbbe3735b12b4a79baeb5c
                self.remove_copy(unit);
            }
            // @lfy def/cli/main.lfy:compile#compile:compile:a758d1de98247fd21db6f348f4b6ad304727df53a1f2655c9c1794be92ac7fc5
            Merge::Undone(why) => self.undone(unit, &why),
        }
    }

    /// What merging a unit done in its copy writes into its change set's copy, or why it is
    /// undone; nothing is left written when it is.
    // @lfy def/cli/main.lfy:compile#compile:compile:89c7d76425815427ea012671f3b4f7dddc0031ff70cc737c12eebf4300797c95
    // @lfy def/cli/main.lfy:compile#compile:compile:e1494d50bc4ca15b22408682181b99b6a82a81da36ad36697a5ce42702ba41d9
    // @lfy def/cli/main.lfy:compile#compile:compile:dd602ad17aa5add15d7e0b72130ebaa6955e268ab8b31ac9d2c28c819c36a028
    // @lfy def/cli/main.lfy:compile#compile:compile:d67feda78f528338c7b27d494e8089387777160911a29fc46173c99bfa1b08fd
    // @lfy def/cli/main.lfy:compile#compile:compile:f2af5d301e941d5cb53fb105a6fa1da10f29d56769455ef770a4dadb9f892796
    // @lfy def/cli/main.lfy:compile#compile:compile:fd9b01ad05607a5a609c83192a3af14b8fadef545c84da493960eca8b0faef6d
    fn merged_into_set(&mut self, unit: usize, edits: &[ForeignEdit]) -> Merge {
        let root = self.root();
        let target = self.target_of_unit(unit).clone();
        let Some(copy) = self.schedule.copies.get(&unit).cloned() else {
            return Merge::Undone("its copy is gone".to_string());
        };
        let Some(set) = self.set_of(unit).cloned() else { return Merge::Merged(Vec::new()) };
        let into = PathBuf::from(&set.directory);
        // A region of its own, or one it made a foreign edit in, that another unit changed
        // since its copy was made leaves nothing of it merged.
        // @lfy def/cli/main.lfy:compile#compile:compile:d67feda78f528338c7b27d494e8089387777160911a29fc46173c99bfa1b08fd
        if let Some(why) = self.changed_region(unit, &copy, &into, &target, edits) {
            return Merge::Undone(why);
        }
        // Each file its copy changed, merged three ways where the change set's copy changed it
        // too; a conflict leaves nothing of it merged.
        // @lfy def/cli/main.lfy:compile#compile:compile:89c7d76425815427ea012671f3b4f7dddc0031ff70cc737c12eebf4300797c95
        // @lfy def/cli/main.lfy:compile#compile:compile:dd602ad17aa5add15d7e0b72130ebaa6955e268ab8b31ac9d2c28c819c36a028
        // @lfy def/cli/main.lfy:compile#compile:compile:f2af5d301e941d5cb53fb105a6fa1da10f29d56769455ef770a4dadb9f892796
        let changes = changed_in(&copy, &target);
        let writes = match merges_of(&changes, &into, &root.join(crate::copies::CHANGES).join("merge")) {
            Ok(writes) => writes,
            Err(path) => return Merge::Undone(format!("{path} did not merge without a conflict")),
        };
        // What each file it writes held before, so that the merge can be undone.
        // @lfy def/cli/main.lfy:compile#compile:compile:fd9b01ad05607a5a609c83192a3af14b8fadef545c84da493960eca8b0faef6d
        let mut held: crate::copies::Held = BTreeMap::new();
        for (path, _, _) in &writes {
            held.insert(path.clone(), held_at(&into, path));
        }
        let wrote_outputs = !writes.is_empty();
        if let Err(path) = write_merges(&writes, &into) {
            put_back(&into, &held);
            return Merge::Undone(format!("{path} could not be written"));
        }
        let mut paths: Vec<String> = writes.iter().map(|(path, _, _)| path.clone()).collect();
        // Its map file and its review record file, as its copy holds them, never merged.
        // @lfy def/cli/main.lfy:compile#compile:compile:e1494d50bc4ca15b22408682181b99b6a82a81da36ad36697a5ce42702ba41d9
        let planned = self.plan.units[unit].clone();
        for file in [
            format!("{MAPS}/{}/{}.json", target.identifier, planned.stem),
            format!("{REVIEWS}/{}/{}.json", target.identifier, planned.stem),
        ] {
            let mine = held_at(&copy.directory, &file);
            let theirs = held_at(&into, &file);
            if mine.as_ref().map(|(text, _)| text) == theirs.as_ref().map(|(text, _)| text) {
                continue;
            }
            held.insert(file.clone(), theirs);
            put_back(&into, &[(file.clone(), mine)].into());
            paths.push(file);
        }
        // The maps of the unit, and of every unit with a marker of its own in a file the merge
        // wrote, re-derived in the change set's copy from what their map files there hold.
        // @lfy def/cli/main.lfy:compile#compile:compile:e1494d50bc4ca15b22408682181b99b6a82a81da36ad36697a5ce42702ba41d9
        let texts: Vec<String> =
            writes.iter().filter_map(|(path, _, _)| fs::read_to_string(into.join(path)).ok()).collect();
        let owners: Vec<usize> = (0..self.plan.units.len())
            .filter(|&other| {
                other == unit
                    || self.plan.units[other].target == planned.target
                        && texts.iter().any(|text| {
                            generation::parse_markers(text).iter().any(|marker| own_marker(self.workspace(), &self.plan, other, marker))
                        })
            })
            .collect();
        for other in owners {
            let was = self.maps_in(other, &into);
            if was.is_empty() {
                continue;
            }
            let plan = self.plan.clone();
            let now = generation::rederive(self.program_at(&into), &plan, other, &was);
            if now == was {
                continue;
            }
            let file = relative(&into, &self.map_file_in(other, &into));
            held.entry(file.clone()).or_insert_with(|| held_at(&into, &file));
            // @lfy def/cli/main.lfy:compile#compile:compile:e1494d50bc4ca15b22408682181b99b6a82a81da36ad36697a5ce42702ba41d9
            if !generation::record(&self.program_at(&into).workspace, &plan.units[other], &now) {
                complain(self.style.err, Some(&file), "could not be written");
                self.worsen(ExitCode::Failure);
            }
            if !paths.contains(&file) {
                paths.push(file);
            }
        }
        // The build, then the test with stubs allowed, in the change set's copy, once the merge
        // wrote or removed a file under the output directory; nothing runs otherwise. One that
        // fails undoes the merge.
        // @lfy def/cli/main.lfy:compile#compile:compile:e97aeb05e9fe84cdc671d0ff5fc5ad164bbca228900888f23cc6a7beedcdb985
        // @lfy def/cli/main.lfy:compile#compile:compile:440bee8e71d6f1c2ae85585e6d67504b3322b68096a7a0167d6acade8f10fde8
        // @lfy def/cli/main.lfy:compile#compile:compile:fd9b01ad05607a5a609c83192a3af14b8fadef545c84da493960eca8b0faef6d
        if wrote_outputs {
            for operation in [Operation::Build, Operation::Test] {
                // @lfy def/cli/main.lfy:compile#compile:compile:2693766a523314183e6282316ef0592ed35595e8a47c47ed91606d938be00563
                if line_for(&target, operation).is_none() {
                    continue;
                }
                let Some(ran) = run_target_command(&target, operation, &into, Some(&self.running), true) else {
                    continue;
                };
                if ran.succeeded() {
                    continue;
                }
                self.could_not_run(&ran);
                // @lfy def/cli/main.lfy:compile#compile:compile:fd9b01ad05607a5a609c83192a3af14b8fadef545c84da493960eca8b0faef6d
                put_back(&into, &held);
                return Merge::Undone(format!("the {} command exited with {}", word_of_operation(operation), ran.code));
            }
        }
        paths.sort();
        paths.dedup();
        Merge::Merged(paths)
    }

    /// Why a unit's merge is undone before anything of it is written: a region of a marker of
    /// its own, or a region it made a foreign edit in, holds other lines in the change set's
    /// copy than its own copy held when it was made; nothing when none does.
    // @lfy def/cli/main.lfy:compile#compile:compile:d67feda78f528338c7b27d494e8089387777160911a29fc46173c99bfa1b08fd
    fn changed_region(&self, unit: usize, copy: &UnitCopy, into: &Path, target: &Target, edits: &[ForeignEdit]) -> Option<String> {
        let stem = self.stem(unit);
        for (path, made) in &copy.made {
            if !is_output(path, target) {
                continue;
            }
            let now = fs::read(into.join(path)).ok();
            if now.as_deref() == Some(made.as_slice()) {
                continue;
            }
            let was = String::from_utf8_lossy(made).into_owned();
            let now = now.map(|held| String::from_utf8_lossy(&held).into_owned()).unwrap_or_default();
            // @lfy def/cli/main.lfy:compile#compile:compile:d67feda78f528338c7b27d494e8089387777160911a29fc46173c99bfa1b08fd
            if own_region_text(self.workspace(), &self.plan, unit, &was) != own_region_text(self.workspace(), &self.plan, unit, &now) {
                return Some(format!("{path}: a region of {stem} was changed by another unit since its copy was made"));
            }
            for edit in edits.iter().filter(|edit| &edit.path == path) {
                if region_lines(&was, &edit.region) != region_lines(&now, &edit.region) {
                    return Some(format!(
                        "{path}: the region of {} that {stem} edited was changed by another unit since its copy was made",
                        edit.owner
                    ));
                }
            }
        }
        None
    }

    /// A merged unit accepted in its change set: its state becomes reviewed when it was
    /// reviewed and accepted otherwise, with the hash and requirements it has now; each of the
    /// foreign edits of its last accepted verdict is appended to the change set's, in order,
    /// its count of edits becoming theirs; and what its merge wrote is kept, as `ChangeSet`
    /// says. The change set's file is written at once, one merged line counts the files its
    /// merge wrote or removed, and the foreign edits then do what they do to the units after
    /// it.
    // @lfy def/cli/main.lfy:compile#compile:compile:55e0af72225acb1c6b134a3811478ff630be96bdc40d1f40970151a84cb1eada
    // @lfy def/cli/main.lfy:compile#compile:compile:77eb0c2c3b5559ce6700a2a65f66921a86b34f2759727cb081ba2ffcc3450fa0
    // @lfy def/cli/main.lfy:compile#compile:compile:3d654e560f2e1144270b416e60c71b73f41429d32f691bba2f8f6fab0038bd7a
    pub(crate) fn accept_merged(&mut self, unit: usize, state: UnitState, edits: Vec<ForeignEdit>, paths: Vec<String>) {
        let root = self.root();
        let stem = self.stem(unit);
        let target = self.target_of_unit(unit).identifier.clone();
        let hash = generation::source_hash(&self.plan.units[unit].text);
        let requirements = generation::source_hash(&requirement_ids_of(&self.program, &self.plan.units[unit]).join("\n"));
        let count = paths.len();
        if let Some(set) = self.sets.get_mut(&target)
            && let Some(place) = set.units.iter().position(|held| held.stem == stem)
        {
            // @lfy def/cli/main.lfy:compile#compile:compile:55e0af72225acb1c6b134a3811478ff630be96bdc40d1f40970151a84cb1eada
            set.accept(place, &hash, &requirements, edits.clone());
            set.units[place].state = state;
            if let Err(error) = set.merge(&root, place, &request_name(&stem), paths) {
                complain(self.style.err, Some(&set.directory), &error.to_string());
                self.worsen(ExitCode::Failure);
            }
        }
        // @lfy def/cli/main.lfy:compile#compile:compile:3d654e560f2e1144270b416e60c71b73f41429d32f691bba2f8f6fab0038bd7a
        self.save(&target);
        self.schedule.merged.insert(unit);
        self.schedule.since_start.insert(unit);
        self.schedule.undone.remove(&unit);
        self.schedule.again.remove(&unit);
        self.count_done();
        // @lfy def/cli/main.lfy:compile#compile:compile:77eb0c2c3b5559ce6700a2a65f66921a86b34f2759727cb081ba2ffcc3450fa0
        let word = if count == 1 { "file" } else { "files" };
        self.reporter.report(Step::Merged, Some(&stem), &format!("{count} {word}"));
        if !edits.is_empty() {
            self.foreign_edits_recorded(unit, &edits);
        }
    }

    /// A unit's merge undone: one undone line says why, and the unit runs again from its
    /// start, as a planned unit, in a new copy of its own made as its change set's copy
    /// stands; its run counts toward none of its retries. A unit whose run before had its
    /// merge undone too stops without being accepted, with that reason as its problem.
    // @lfy def/cli/main.lfy:compile#compile:compile:a758d1de98247fd21db6f348f4b6ad304727df53a1f2655c9c1794be92ac7fc5
    // @lfy def/cli/main.lfy:compile#compile:compile:91514d592807a6eae69b04199835ae037c731478013def307e41bb393d1f98c3
    // @lfy def/cli/main.lfy:compile#compile:compile:ee6f110a6e8a80fe7f058125db6f1cd190ee42098edb6f0deaed19ccf0a70a19
    pub(crate) fn undone(&mut self, unit: usize, why: &str) {
        let stem = self.stem(unit);
        // @lfy def/cli/main.lfy:compile#compile:compile:ee6f110a6e8a80fe7f058125db6f1cd190ee42098edb6f0deaed19ccf0a70a19
        if self.schedule.undone.contains(&unit) {
            self.reporter.report_problems(Step::Undone, Some(&stem), why, &[why.to_string()]);
            self.stopped_unit(unit, Stop::Undone);
            return;
        }
        // @lfy def/cli/main.lfy:compile#compile:compile:a758d1de98247fd21db6f348f4b6ad304727df53a1f2655c9c1794be92ac7fc5
        self.reporter.report(Step::Undone, Some(&stem), why);
        self.remove_copy(unit);
        self.schedule.undone.insert(unit);
        self.schedule.again.insert(unit);
    }

    /// One unit's copy removed, once nothing more is read from it.
    // @lfy def/cli/main.lfy:compile#compile:compile:38e29db3efde6fef8e4f2e633ff44b5dee097d14f8bbbe3735b12b4a79baeb5c
    pub(crate) fn remove_copy(&mut self, unit: usize) {
        if let Some(copy) = self.schedule.copies.remove(&unit) {
            let _ = remove_directory(&copy.directory);
        }
    }

    /// Every copy of a unit still kept removed, as when the compile stops or every unit that
    /// could merge has.
    // @lfy def/cli/main.lfy:compile#compile:compile:38e29db3efde6fef8e4f2e633ff44b5dee097d14f8bbbe3735b12b4a79baeb5c
    pub(crate) fn remove_every_copy(&mut self) {
        let units: Vec<usize> = self.schedule.copies.keys().copied().collect();
        for unit in units {
            self.remove_copy(unit);
        }
        self.schedule.done.clear();
    }

    /// One unit worked from its first run to its outcome in its copy: the compiler, the
    /// verifier, and the target's commands all run for it there, and what it comes to is done
    /// in its copy, to merge, or a stop.
    ///
    /// Its first run may skip the compiler, as a reworded-only, a dependency-only, or a
    /// touched-fix unit's may; a skipped run counts as no run of the compiler in any count that
    /// decides whether the unit is run once more or stops, and anything that goes wrong with it
    /// sends the unit to the compiler as its first run. The counts are of the runs since the
    /// copy was made.
    // @lfy def/cli/main.lfy:compile#compile:compile:649dfaca80ede5a5de3cfecd48f1de664e250e9bc65773266822f3f63ae34c71
    // @lfy def/cli/main.lfy:compile#compile:compile:3a5a6187176db8b0b5e61a117d0b28b3181489ed6ba22acc61e795ac4bbf9d31
    // @lfy def/cli/main.lfy:compile#compile:compile:1672da5d763ee2c0c8a1a0e012ba1eb3a94200faedfbafa2b5a74254c696bfb6
    // @lfy def/cli/main.lfy:compile#compile:compile:91514d592807a6eae69b04199835ae037c731478013def307e41bb393d1f98c3
    pub(crate) fn work_unit(&mut self, unit: usize, command: &str, directory: &Path) -> Ended {
        let stem = self.stem(unit);
        let reason = self.plan.units[unit].reason.map_or_else(|| "up to date".to_string(), |r| r.as_str().to_string());
        // What may stand in for the compiler is decided before any marker of the unit's
        // outputs is rewritten: the outputs on disk must be exactly the ones the last
        // acceptance recorded.
        // @lfy def/cli/main.lfy:compile#compile:compile:80966c9bedadf4c34910a903c33a537e23616ffb635289db0d2d5adc02ef1aa7
        // @lfy def/cli/main.lfy:compile#compile:compile:f82d76ff8e23e283093211b99bd7fa4f7edfa0a9f9151cf64b7864379ceec690
        let stand = self.outputs_stand(unit, directory);
        self.reporter.report(Step::Requesting, Some(&stem), &reason);
        // Before each request, the markers of the unit's reworded criteria and tests name the
        // ids they have now.
        // @lfy def/cli/main.lfy:compile#compile:compile:df256f27e32c909f4105584820a74d3c0a84eb64ec1be799e6387232cf66057e
        self.reword_outputs(unit, directory);
        // The request is given, as the unit's foreign edits, those recorded since it was last
        // accepted in its change set that a unit its copy holds the merge of made.
        // @lfy def/cli/main.lfy:compile#compile:compile:d9ff66213993aa0d26ba013faf314fca87796340ce0cfd4bbd4da57f20bbf6ce
        let edits: Vec<ForeignEdit> = self.edits.get(&unit).cloned().unwrap_or_default();
        // @lfy def/cli/main.lfy:compile#compile:compile:3a5a6187176db8b0b5e61a117d0b28b3181489ed6ba22acc61e795ac4bbf9d31
        let (mut request, accepted_source) = self.request_of(unit, directory, true, &edits);
        let mut stage = self.first_stage(unit, &request, stand, accepted_source);
        let runs = Runs::new(self.agent.clone(), self.running.clone());
        let mut tries = Tries::default();
        // What the unit's first review carries: what it was last reviewed as, in its
        // directory.
        // @lfy def/cli/main.lfy:compile#compile:compile:291055dad770c8b6cca4ad39374f2decfc9ad7211f3a98cda19486467b3c72be
        let mut records = self.records_of(unit, directory);
        // The files of the output directory just before the first run of the compiler since
        // the copy was made.
        // @lfy def/cli/main.lfy:compile#compile:compile:7d2892039e629efc80e06fa9613480a7affbe990c13c5aeb53877f888c21b969
        let mut before: Option<Vec<Output>> = None;
        loop {
            // The compile stopped: nothing more runs, and what this gives is ignored.
            // @lfy def/cli/main.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
            if self.running.is_stopped() {
                return Ended::Stopped(Stop::Failed);
            }
            let (verdict, compiled) = match stage {
                Stage::Compiler => match self.compile_once(unit, command, directory, &request, &runs, &mut tries, &mut before) {
                    Attempt::Verdict(verdict) => (verdict, true),
                    Attempt::Again(problems) => {
                        append_problems(&mut request, &problems);
                        continue;
                    }
                    Attempt::Stopped(stop) => return Ended::Stopped(stop),
                },
                // The unit gate stands in for the compiler, and the unit is then accepted or
                // rejected as after a run of it.
                // @lfy def/cli/main.lfy:compile#compile:compile:4666d4f1b1a16d4d3052ff2b5c6f1d66c8ccc6a8404f455f4ded9c61d0d52067
                // @lfy def/cli/main.lfy:compile#compile:compile:ce28b2a5274a7561b5410f710bf240bf388a32b2bcbc463520600896aeb19e78
                Stage::Dependency => {
                    let maps = self.maps_in(unit, directory);
                    if let Err(problem) = self.unit_gate(unit, &maps, directory) {
                        // @lfy def/cli/main.lfy:compile#compile:compile:ce28b2a5274a7561b5410f710bf240bf388a32b2bcbc463520600896aeb19e78
                        self.report_gate(unit, &problem, false);
                        stage = Stage::Compiler;
                        continue;
                    }
                    let now = files_in(self.target_of_unit(unit), directory);
                    let verdict = self.verdict_of(unit, &request, directory, Some(&now), &BTreeMap::new());
                    if !verdict.accepted {
                        // @lfy def/cli/main.lfy:compile#compile:compile:ce28b2a5274a7561b5410f710bf240bf388a32b2bcbc463520600896aeb19e78
                        let _turn = self.reporter.hold();
                        self.report_rejection(unit, &verdict.problems);
                        self.reporter.report(Step::Building, Some(&stem), "the unit was rejected after its gate passed");
                        stage = Stage::Compiler;
                        continue;
                    }
                    // The unit is accepted on its unit gate: its outputs are written back, no
                    // verifier runs on it, its maps are recorded, no review record file
                    // changes, and it is done in its copy, to merge reviewed.
                    // @lfy def/cli/main.lfy:compile#compile:compile:df7809a6c14fcad6802578626735a0445fb4c6dcb342819ac8fcc43ac598cafc
                    if !self.write_back(unit, &verdict, directory) {
                        return Ended::Stopped(Stop::Failed);
                    }
                    self.reporter.report(Step::Accepted, Some(&stem), &format!("{} outputs", verdict.outputs.len()));
                    // @lfy def/cli/main.lfy:compile#compile:compile:df7809a6c14fcad6802578626735a0445fb4c6dcb342819ac8fcc43ac598cafc
                    self.record_maps(unit, &verdict.source_maps, directory, false);
                    return Ended::Done(UnitState::Reviewed, verdict.foreign_edits);
                }
                // The outputs are checked as they are, with every file of the listing as it is
                // now as the files before the run, and with no notes.
                // @lfy def/cli/main.lfy:compile#compile:compile:80966c9bedadf4c34910a903c33a537e23616ffb635289db0d2d5adc02ef1aa7
                // @lfy def/cli/main.lfy:compile#compile:compile:b44cd9235b564a50b5d58b80e928bf32262b3d5b69b8aad8ef60c1bdcfa59259
                Stage::Reworded | Stage::Fixes => {
                    let now = files_in(self.target_of_unit(unit), directory);
                    let verdict = self.verdict_of(unit, &request, directory, Some(&now), &BTreeMap::new());
                    if !verdict.accepted {
                        // The problems are printed and the compiler runs on the unit as its
                        // first run; the rejection counts toward no retry.
                        // @lfy def/cli/main.lfy:compile#compile:compile:4f28bce06fb2870b28cb51c505e5fc1c33b4a5834d9c1680e3ff4d797d43ec28
                        // @lfy def/cli/main.lfy:compile#compile:compile:ba06a4655e253030d84c2c80b9e79c771fd19cd90fe47216e277cabfc5e530d3
                        self.report_rejection(unit, &verdict.problems);
                        if stage == Stage::Fixes {
                            append_problems(&mut request, &verdict.problems);
                        }
                        stage = Stage::Compiler;
                        continue;
                    }
                    (verdict, false)
                }
            };
            // The outcome is accepted: the outputs are written back in the unit's directory
            // and the unit gate runs on them.
            // @lfy def/cli/main.lfy:compile#compile:compile:76969599fbfc689601061f3ce4424ede9d19cda1bf9b138f51bf96e26a229348
            // @lfy def/cli/main.lfy:compile#compile:compile:5bdebf278eae36f6a91cb9b544d6b8e89fef337d885f3d2dbcdc7cde773503ec
            // @lfy def/cli/main.lfy:compile#compile:compile:e3d4906500c6a0dc5bca8cb0c0f72d9d30c6afb8e30feb4410b663fb8a31b5f5
            if !self.write_back(unit, &verdict, directory) {
                return Ended::Stopped(Stop::Failed);
            }
            if let Err(problem) = self.unit_gate(unit, &verdict.source_maps, directory) {
                // @lfy def/cli/main.lfy:compile#compile:compile:4f08b908abe37e634c11aec194dccf59c1b1070cffacbf4ac29aeed36ffbb31f
                self.report_gate(unit, &problem, true);
                let problems = vec![problem];
                // A first run that skipped the compiler sends the unit to the compiler as its
                // first run, counting toward no retry.
                // @lfy def/cli/main.lfy:compile#compile:compile:4f28bce06fb2870b28cb51c505e5fc1c33b4a5834d9c1680e3ff4d797d43ec28
                // @lfy def/cli/main.lfy:compile#compile:compile:ba06a4655e253030d84c2c80b9e79c771fd19cd90fe47216e277cabfc5e530d3
                if !compiled {
                    if stage == Stage::Fixes {
                        append_problems(&mut request, &problems);
                    }
                    stage = Stage::Compiler;
                    continue;
                }
                // The first time, the unit is run once more with the problems appended; the
                // second time, it is rejected after its retries.
                // @lfy def/cli/main.lfy:compile#compile:compile:4f06b5aa3049fb148a394aa3261e0dc49c2e9cb564b4604bea58741f2a16204f
                // @lfy def/cli/main.lfy:compile#compile:compile:466c1564df1c6196c4187bb8cee949205482091b548db1a7939a7ef1478b2dec
                if tries.rejected {
                    return Ended::Stopped(Stop::Rejected);
                }
                tries.rejected = true;
                tries.resume_compiler = true;
                append_problems(&mut request, &problems);
                continue;
            }
            self.reporter.report(Step::Accepted, Some(&stem), &format!("{} outputs", verdict.outputs.len()));
            // No verifier, or --no-verify: the maps are recorded in the unit's directory and
            // the unit is done in its copy, to merge accepted.
            // @lfy def/cli/main.lfy:compile#compile:compile:55b0f231497b975a7352cc15265c4295dfcd3153f5a093823323dc778bae3913
            // @lfy def/cli/main.lfy:compile#compile:compile:1230241ea758cb06bf3795f8f7d1593c403fd5ebec62a0b8090b7de98aa70aae
            if !self.verifies() {
                self.record_maps(unit, &verdict.source_maps, directory, false);
                return Ended::Done(UnitState::Accepted, verdict.foreign_edits);
            }
            // The unit is verified.
            // @lfy def/cli/main.lfy:compile#compile:compile:e043cf95bc9cd594a76db11930ff69d434f4eafa3b00576563e48b0caf192912
            // @lfy def/cli/main.lfy:compile#compile:compile:2e4acb5558efbdd9e38f447b9d1f843cbed7458fa84833e158b3d50441db7334
            match self.verify_unit(unit, command, directory, &verdict.source_maps, &records, &runs, tries.resume_verifier) {
                Verified::Passed => {
                    // @lfy def/cli/main.lfy:compile#compile:compile:9854e85deefe68148920771ac99fcfeab835dc084af3382a76328d6730b88e94
                    // @lfy def/cli/main.lfy:compile#compile:compile:14cbfe6603ade5c97b980e258fee2c6beaff3dce14dd1ea1d7e29c34507da39f
                    self.record_maps(unit, &verdict.source_maps, directory, true);
                    return Ended::Done(UnitState::Reviewed, verdict.foreign_edits);
                }
                Verified::Stopped => return Ended::Stopped(Stop::Failed),
                Verified::Violated { problems, next } => {
                    // The unit stays planned in its change set while it runs again.
                    // @lfy def/cli/main.lfy:compile#compile:compile:836ff964045c3fdf6d9f590423c540dcc1814918a94ca6164f5840c078d87342
                    // The second such report rejects the unit after its retries, and its maps
                    // are not recorded.
                    // @lfy def/cli/main.lfy:compile#compile:compile:a8d0e0a443a2adbf0143baf22ffaa2b0ec6dc8b39fb3efe5c9ec8d26eee79b87
                    if tries.violated {
                        return Ended::Stopped(Stop::Rejected);
                    }
                    // The first runs the unit once more with the problems appended, even when it
                    // was already run once more for a rejected outcome: the compiler's first
                    // run when it has not run on the unit yet, and a resuming run when it has.
                    // @lfy def/cli/main.lfy:compile#compile:compile:989f6f309c8ec4cc4f9e9682676ed8016bcfaac8a5f6ccf14d33c1bc0207a0ec
                    // @lfy def/cli/main.lfy:compile#compile:compile:ec0e37d5ec1e376b65aa90a57f20654ed0153dcd1c1cb0899bf5f1b988e4c2ce
                    // The verifier's run once more resumes only after a resuming run of the
                    // compiler, which there is only when the compiler ran on the unit before.
                    // @lfy def/cli/main.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
                    // @lfy def/cli/main.lfy:compile#compile:compile:9e092cdf07f0cd7a1468317b2e75bb10ee5513015372fa964ca1fdff2d436dba
                    tries.violated = true;
                    tries.resume_compiler = tries.compiled > 0;
                    tries.resume_verifier = tries.compiled > 0;
                    records = next;
                    append_problems(&mut request, &problems);
                    stage = Stage::Compiler;
                }
            }
        }
    }

    /// What a unit's first run is: the compiler, unless something may stand in for it.
    ///
    /// A reworded-only unit has the markers of those criteria and tests moved and its outputs
    /// checked, which asks no compiler, but only while a verifier may look at the result, its
    /// request carries no foreign edit, its outputs stand, and its previous source is defined
    /// and not only its hash-matched source. A unit with the reason dependency has the unit
    /// gate run instead, while the target has a line for build and for test, its request
    /// carries no foreign edit, and its outputs stand. A touched unit whose foreign edits are
    /// all fixes is checked as it is. Every other unit, one touched by a stub among them, goes
    /// to the compiler.
    // @lfy def/cli/main.lfy:compile#compile:compile:80966c9bedadf4c34910a903c33a537e23616ffb635289db0d2d5adc02ef1aa7
    // @lfy def/cli/main.lfy:compile#compile:compile:10b9ec9d79a2f879f1f76e724fef7226300dd6b087d5accf4ca2f46f5102e90b
    // @lfy def/cli/main.lfy:compile#compile:compile:f82d76ff8e23e283093211b99bd7fa4f7edfa0a9f9151cf64b7864379ceec690
    // @lfy def/cli/main.lfy:compile#compile:compile:343acafbb0e354bca79a21aac598241ee5886911dba44bf7b3766bec69054122
    // @lfy def/cli/main.lfy:compile#compile:compile:b44cd9235b564a50b5d58b80e928bf32262b3d5b69b8aad8ef60c1bdcfa59259
    // @lfy def/cli/main.lfy:compile#compile:compile:8214a980d0dc5f7d2ad165f456c7096d6e4ef89966519c6a783ecc99a04969fd
    // @lfy def/cli/main.lfy:compile#compile:compile:d43663ac22c59c11af52567c184b43ae8ce9a8ccec6837a4f398f5a82a41c8e5
    // @lfy def/cli/main.lfy:compile#compile:compile:92dda467736030409d0aa3d11b09f0e0dfe83df48a8098c9b062fb817c2ed45e
    pub(crate) fn first_stage(&self, unit: usize, request: &Request, stand: bool, accepted_source: bool) -> Stage {
        let reason = self.plan.units[unit].reason;
        let edits = &request.foreign_edits;
        // @lfy def/cli/main.lfy:compile#compile:compile:b44cd9235b564a50b5d58b80e928bf32262b3d5b69b8aad8ef60c1bdcfa59259
        if reason == Some(Reason::Touched)
            && !edits.is_empty()
            && edits.iter().all(|edit| edit.kind == ForeignEditKind::Fix)
        {
            return Stage::Fixes;
        }
        // An output that is no longer what was accepted, or a foreign edit in it, leaves the
        // compiler to do.
        // @lfy def/cli/main.lfy:compile#compile:compile:92dda467736030409d0aa3d11b09f0e0dfe83df48a8098c9b062fb817c2ed45e
        if !stand || !edits.is_empty() {
            return Stage::Compiler;
        }
        // @lfy def/cli/main.lfy:compile#compile:compile:80966c9bedadf4c34910a903c33a537e23616ffb635289db0d2d5adc02ef1aa7
        if request.reworded_only && self.verifies() && accepted_source {
            return Stage::Reworded;
        }
        // @lfy def/cli/main.lfy:compile#compile:compile:f82d76ff8e23e283093211b99bd7fa4f7edfa0a9f9151cf64b7864379ceec690
        if reason == Some(Reason::Dependency) && self.builds_and_tests(unit) {
            return Stage::Dependency;
        }
        // @lfy def/cli/main.lfy:compile#compile:compile:d43663ac22c59c11af52567c184b43ae8ce9a8ccec6837a4f398f5a82a41c8e5
        Stage::Compiler
    }

    /// One run of the compiler on a unit and what its outcome comes to: a verdict to go on
    /// with when it is accepted, the problems to run once more with, or how the unit stopped.
    ///
    /// The run is one stream of the compiler command, with Request.instructions as its input
    /// and the unit's directory as its directory, bound to the request and the files before
    /// the unit's first run. The unit is accepted or rejected by the verdict of `accept` on
    /// its outputs there, with the notes the run gave the units it touched, and how the run
    /// ended is `outcomeOf` the report, that verdict, and what the run submitted.
    // @lfy def/cli/main.lfy:compile#compile:compile:3a5a6187176db8b0b5e61a117d0b28b3181489ed6ba22acc61e795ac4bbf9d31
    // @lfy def/cli/main.lfy:compile#compile:compile:6d22af1c9b1faa80650ee222bf29ec59916fad170472c864b3ea67c7206c69c9
    // @lfy def/cli/main.lfy:compile#compile:compile:b28eabe38d6ecf240b63365cf837bbf5f8e09844c871b6fc2b6f34745cc9c31c
    // @lfy def/cli/main.lfy:compile#compile:compile:37cff27bf8d7f9a2a7c28ad0e80bf42ae1dd4d15a6e7fcf1c687af211003841c
    #[expect(clippy::too_many_arguments, reason = "one run of the compiler on a unit takes all of this")]
    pub(crate) fn compile_once(
        &mut self,
        unit: usize,
        command: &str,
        directory: &Path,
        request: &Request,
        runs: &Runs,
        tries: &mut Tries,
        before: &mut Option<Vec<Output>>,
    ) -> Attempt {
        let stem = self.stem(unit);
        tries.compiled += 1;
        let step = if tries.compiled == 1 { Step::Compiling } else { Step::Retrying };
        self.reporter.report(step, Some(&stem), command);
        // The unit's files before its first run of the compiler since its copy was made, as
        // they read in its directory just then.
        // @lfy def/cli/main.lfy:compile#compile:compile:7d2892039e629efc80e06fa9613480a7affbe990c13c5aeb53877f888c21b969
        let earlier = before.get_or_insert_with(|| files_in(self.target_of_unit(unit), directory)).clone();
        // The run has the role compiler, the unit, the request its input was made from, the
        // files before its first run, no review, and no source maps.
        // @lfy def/cli/main.lfy:compile#compile:compile:6d22af1c9b1faa80650ee222bf29ec59916fad170472c864b3ea67c7206c69c9
        let binding = self.binding_of(Role::Compiler, Some(unit), directory, Some(request.clone()), Some(earlier.clone()), None, Vec::new());
        // @lfy def/cli/main.lfy:compile#compile:compile:3a5a6187176db8b0b5e61a117d0b28b3181489ed6ba22acc61e795ac4bbf9d31
        let exit = runs.run(command, directory, &stem, &request.instructions, binding, tries.resume_compiler);
        // An agent server that could not be hosted runs nothing more: the compile stops, and
        // the code is failure.
        // @lfy def/cli/main.lfy:compile#compile:compile:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
        if self.agent.server.failed() {
            self.reporter.report_problems(Step::Failed, Some(&stem), exit.stderr.trim(), &[exit.stderr.trim().to_string()]);
            complain(self.style.err, None, exit.stderr.trim());
            self.worsen(ExitCode::Failure);
            self.halted = true;
            return Attempt::Stopped(Stop::Failed);
        }
        let outcome = if exit.code == -1 {
            // The code is -1 because the command could not be started or was ended: the
            // outcome is failed.
            // @lfy def/cli/main.lfy:compile#compile:compile:c836eb712f2d9528c78321a4094d806dd4bf34577eaf2bab287bf88d3e18d9f5
            Outcome {
                kind: OutcomeKind::Failed,
                message: format!("the compiler command could not be run: {}", exit.stderr.trim()),
                verdict: None,
            }
        } else {
            // Process.Exit.stdout is the report; each of its lines reached the log as it was
            // shown, so nothing more of it is appended.
            // @lfy def/cli/main.lfy:main#main:main:3082965661f2495d1e5e9d8a2177fc2ca759320db1b83c7e7bc0605b9be65f3f
            // @lfy def/cli/main.lfy:compile#compile:compile:b28eabe38d6ecf240b63365cf837bbf5f8e09844c871b6fc2b6f34745cc9c31c
            let verdict = self.verdict_of(unit, request, directory, Some(&earlier), &exit.record.touches);
            // @lfy def/cli/main.lfy:compile#compile:compile:37cff27bf8d7f9a2a7c28ad0e80bf42ae1dd4d15a6e7fcf1c687af211003841c
            generation::outcome_of(&exit.stdout, Some(verdict), exit.record.submission.as_ref())
        };
        match outcome.kind {
            OutcomeKind::Accepted => match outcome.verdict {
                Some(verdict) => Attempt::Verdict(verdict),
                None => Attempt::Stopped(Stop::Failed),
            },
            // The problems are printed; the first time the unit is run once more with them,
            // and the second time it is rejected after its retries.
            // @lfy def/cli/main.lfy:compile#compile:compile:4f08b908abe37e634c11aec194dccf59c1b1070cffacbf4ac29aeed36ffbb31f
            // @lfy def/cli/main.lfy:compile#compile:compile:4f06b5aa3049fb148a394aa3261e0dc49c2e9cb564b4604bea58741f2a16204f
            // @lfy def/cli/main.lfy:compile#compile:compile:466c1564df1c6196c4187bb8cee949205482091b548db1a7939a7ef1478b2dec
            OutcomeKind::Rejected => {
                let problems = outcome.verdict.map(|verdict| verdict.problems).unwrap_or_else(|| vec![outcome.message.clone()]);
                self.report_rejection(unit, &problems);
                if tries.rejected {
                    return Attempt::Stopped(Stop::Rejected);
                }
                tries.rejected = true;
                tries.resume_compiler = true;
                Attempt::Again(problems)
            }
            // The reason is printed and written, and the unit stops blocked.
            // @lfy def/cli/main.lfy:compile#compile:compile:4c98d6d4f40bffabfc6190ec4effb276a795714b1444ab415ca9d76ab263cb15
            // @lfy def/cli/main.lfy:compile#compile:compile:f2cd308323a1ddb4160c2646b3344c683cf4636e80cf8a2d14cf1ebb99940463
            // @lfy def/cli/main.lfy:compile#compile:compile:4fe0416e02428c995fdf133e17f1ebff400dab93df1ab04b67d3638ed4901499
            OutcomeKind::Blocked => {
                self.reporter.report_reason(Step::Blocked, Some(&stem), &outcome.message, &outcome.message);
                let note = format!("# The compiler is blocked on the unit {stem}\n\n{}\n", outcome.message);
                self.write_note(&stem, "blocked.md", &note);
                Attempt::Stopped(Stop::Blocked)
            }
            // The question is printed and written, and the unit stops with a question.
            // @lfy def/cli/main.lfy:compile#compile:compile:a89d61ad206772d24926bbe963e1a7a9d2ebfc9bc303cd3e8142a6cf11f04dde
            // @lfy def/cli/main.lfy:compile#compile:compile:346b7397a5c634165323cd967d3b1d1db8ef9b2acf33fa25c95a41ae05f695f6
            // @lfy def/cli/main.lfy:compile#compile:compile:5238951562324d9b6e247f27dd2fde2b3d7ba7e2ea67e0c145dce9a0e27d4489
            OutcomeKind::Clarification => {
                self.reporter.report_reason(Step::Clarification, Some(&stem), &outcome.message, &outcome.message);
                let note = format!(
                    "# The compiler asked about the unit {stem}\n\n{}\n\n{ANSWER_HEADING}\n\n<!-- Answer by editing the definitions, or write the answer below this line; the next compile of this unit appends it to the instructions. -->\n",
                    outcome.message
                );
                self.write_note(&stem, "question.md", &note);
                Attempt::Stopped(Stop::Question)
            }
            // The first time the unit is run once more, as no resuming run; the second time it
            // stops failed.
            // @lfy def/cli/main.lfy:compile#compile:compile:c836eb712f2d9528c78321a4094d806dd4bf34577eaf2bab287bf88d3e18d9f5
            // @lfy def/cli/main.lfy:compile#compile:compile:65e040b99a75c39b4cbc0f3005e29d80c5a7ebd77a30ae4ff9e4cd679a4625c5
            OutcomeKind::Failed => {
                self.reporter.report_problems(Step::Failed, Some(&stem), &outcome.message, std::slice::from_ref(&outcome.message));
                if exit.code == -1 {
                    self.worsen(ExitCode::Failure);
                }
                if tries.failed {
                    return Attempt::Stopped(Stop::Failed);
                }
                tries.failed = true;
                // @lfy def/cli/main.lfy:compile#compile:compile:9e092cdf07f0cd7a1468317b2e75bb10ee5513015372fa964ca1fdff2d436dba
                tries.resume_compiler = false;
                Attempt::Again(Vec::new())
            }
        }
    }

    /// A unit kept accepted in a resumed change set verified in the change set's copy before
    /// any unit of the change set starts, as after its unit gate passed, with the source maps
    /// `accept` derives for its outputs as they are: reviewed when the review passes, and
    /// planned again otherwise, so that it runs as any planned unit does. How it stopped, when
    /// the verifier could not be run.
    // @lfy def/cli/main.lfy:compile#compile:compile:e6f63c4110436ce781da7a0d7e40aa3ae7cd30fdadbe53ba4e4743073c700a40
    pub(crate) fn verify_kept(&mut self, unit: usize, command: &str) -> Option<Stop> {
        // @lfy def/cli/main.lfy:compile#compile:compile:0e1f26aad48b4a2e79096c71738d1754b0ac48ecec6dc143f7bb9c5a1e48987d
        self.recover(unit);
        // @lfy def/cli/main.lfy:compile#compile:compile:e6f63c4110436ce781da7a0d7e40aa3ae7cd30fdadbe53ba4e4743073c700a40
        let directory = self.working_directory(unit);
        let stem = self.stem(unit);
        let edits: Vec<ForeignEdit> =
            self.set_of(unit).map(|set| set.edits_since(&stem).into_iter().cloned().collect()).unwrap_or_default();
        let (request, _) = self.request_of(unit, &directory, true, &edits);
        let now = files_in(self.target_of_unit(unit), &directory);
        let verdict = self.verdict_of(unit, &request, &directory, Some(&now), &BTreeMap::new());
        if !verdict.accepted {
            self.set_state(unit, UnitState::Planned);
            return None;
        }
        let runs = Runs::new(self.agent.clone(), self.running.clone());
        let records = self.records_of(unit, &directory);
        match self.verify_unit(unit, command, &directory, &verdict.source_maps, &records, &runs, false) {
            Verified::Passed => {
                self.record_maps(unit, &verdict.source_maps, &directory, true);
                self.set_state(unit, UnitState::Reviewed);
                None
            }
            Verified::Stopped => Some(Stop::Failed),
            // A violated review runs the unit again, as the first run of the compiler on it.
            // @lfy def/cli/main.lfy:compile#compile:compile:ec0e37d5ec1e376b65aa90a57f20654ed0153dcd1c1cb0899bf5f1b988e4c2ce
            Verified::Violated { .. } => {
                self.set_state(unit, UnitState::Planned);
                None
            }
        }
    }

    /// A unit that stopped without being accepted: rejected after its retries, blocked, with
    /// a question, failed a second time, or with its merge undone twice in a row.
    ///
    /// Nothing of its copy is merged into the change set: every file under the target's
    /// output directory that its copy holds otherwise than when the copy was made, or added
    /// since, is written as its copy holds it at its path relative to the copy under
    /// `elfie-requests/<name>.rejected` under the root, and the copy is removed. A rejected
    /// unit's state is rejected, and every other's stays planned. Without --continue the compile
    /// stops; with it, every unit that depends on this one is skipped.
    // @lfy def/cli/main.lfy:compile#compile:compile:6e192fbbaa3d541cc1fa9490c15245bc628f83977d91727e7004d801f77b9104
    // @lfy def/cli/main.lfy:compile#compile:compile:0954f463a673ef9da817b979310906a5a5009931bd64cc4941c7822e82a8d017
    // @lfy def/cli/main.lfy:compile#compile:compile:654d815adda3e49a4d09b56d2ccda6d531ddada64a727f523fcd7ed1b5ff4b9b
    // @lfy def/cli/main.lfy:compile#compile:compile:9051d3b21e94dd454bcd1cc00024dba85e18ea05da69961602354cc8d1b436bc
    pub(crate) fn stopped_unit(&mut self, unit: usize, stop: Stop) {
        match stop {
            Stop::Rejected => self.rejected += 1,
            Stop::Blocked | Stop::Question => self.blocked += 1,
            Stop::Failed | Stop::Undone => self.failed += 1,
        }
        // @lfy def/cli/main.lfy:compile#compile:compile:3d1e2128231c0dd4a623216ed6845b9bfdce55b3a624742046eb528f6cff68dd
        self.worsen(ExitCode::Problems);
        // @lfy def/cli/main.lfy:compile#compile:compile:6e192fbbaa3d541cc1fa9490c15245bc628f83977d91727e7004d801f77b9104
        if let Some(copy) = self.schedule.copies.get(&unit).cloned() {
            self.keep_rejected(unit, &copy);
        }
        self.remove_copy(unit);
        // @lfy def/cli/main.lfy:compile#compile:compile:0954f463a673ef9da817b979310906a5a5009931bd64cc4941c7822e82a8d017
        // @lfy def/cli/main.lfy:compile#compile:compile:654d815adda3e49a4d09b56d2ccda6d531ddada64a727f523fcd7ed1b5ff4b9b
        let state = if stop == Stop::Rejected { UnitState::Rejected } else { UnitState::Planned };
        self.set_state(unit, state);
        self.stopped.insert(unit);
        // Without --continue the compile stops: no unit of any target starts or merges after
        // it, and no final gate, global review, or merge into the root runs.
        // @lfy def/cli/main.lfy:compile#compile:compile:9051d3b21e94dd454bcd1cc00024dba85e18ea05da69961602354cc8d1b436bc
        if !self.continuing {
            self.halted = true;
            // Every run still going is stopped, with every process of its group.
            // @lfy def/cli/main.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
            self.running.stop();
        }
    }

    /// Every file under the target's output directory that a stopped unit's copy holds
    /// otherwise than when the copy was made, or added since, written as the copy holds it at
    /// its path relative to the copy under `elfie-requests/<name>.rejected` under the root.
    // @lfy def/cli/main.lfy:compile#compile:compile:6e192fbbaa3d541cc1fa9490c15245bc628f83977d91727e7004d801f77b9104
    pub(crate) fn keep_rejected(&mut self, unit: usize, copy: &UnitCopy) {
        let target = self.target_of_unit(unit).clone();
        let kept = self.requests_directory().join(format!("{}.rejected", request_name(&self.stem(unit))));
        for (path, change) in changed_in(copy, &target) {
            let Some(text) = change.text else { continue };
            let to = kept.join(&path);
            // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
            let written = to.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(&to, text));
            if let Err(error) = written {
                complain(self.style.err, Some(&to.display().to_string()), &error.to_string());
            }
        }
    }

    /// The foreign edits of a merged unit recorded: the plan is made again as the first plan
    /// was, with the foreign edits recorded since their owner was last accepted in place of
    /// those it was given, so its order is the same. Each owner not yet in the change set
    /// joins it at its place in plan order with the reason touched; an owner already accepted
    /// or reviewed is planned again with the reason the plan gives it. Either is planned during
    /// the compile, so it starts once every planned unit before it has merged. One progress
    /// line then follows for each unit the edits were made in, in plan order: stubbed when one
    /// of the edits made in it is a stub, and touched otherwise.
    // @lfy def/cli/main.lfy:compile#compile:compile:fc8d9730ea95e71cb418a2d3c7249b91a9f7ca20d5041d67b3296632614122c8
    // @lfy def/cli/main.lfy:compile#compile:compile:eeb6eb51ce2fa61105a263d0328d2874ebfc0bc3ced175501ee78c99422ed3ec
    // @lfy def/cli/main.lfy:compile#compile:compile:832c4ab9f9106d5819645657a00f8b70a71f5d25244ab1212be34cffa6e9bbd1
    // @lfy def/cli/main.lfy:compile#compile:compile:186f5bf02a1197f42b15d46a728b008732f742e22b0a3360bbff586dc9f0dcab
    pub(crate) fn foreign_edits_recorded(&mut self, unit: usize, edits: &[ForeignEdit]) {
        let by = self.stem(unit);
        let target = self.target_of_unit(unit).identifier.clone();
        // @lfy def/cli/main.lfy:compile#compile:compile:fc8d9730ea95e71cb418a2d3c7249b91a9f7ca20d5041d67b3296632614122c8
        self.plan_again();
        let owners: Vec<usize> = (0..self.plan.units.len())
            .filter(|&index| {
                let held = &self.plan.units[index];
                target_of(&self.program.workspace, held) == target && edits.iter().any(|edit| edit.owner == held.stem)
            })
            .collect();
        for &owner in &owners {
            let planned = self.plan.units[owner].clone();
            let order: Vec<String> = self.plan.units.iter().map(|held| held.stem.clone()).collect();
            let Some(set) = self.sets.get_mut(&target) else { continue };
            match set.units.iter().position(|held| held.stem == planned.stem) {
                // The owner joins the change set at its place in plan order, as touched.
                // @lfy def/cli/main.lfy:compile#compile:compile:eeb6eb51ce2fa61105a263d0328d2874ebfc0bc3ced175501ee78c99422ed3ec
                None => {
                    let at = set
                        .units
                        .iter()
                        .position(|held| {
                            order.iter().position(|stem| *stem == held.stem) > order.iter().position(|stem| *stem == planned.stem)
                        })
                        .unwrap_or(set.units.len());
                    set.units.insert(at, ChangeSetUnit::planned(&planned.stem, Reason::Touched, "", ""));
                    // @lfy def/cli/main.lfy:compile#compile:compile:9fb81fa1507e767993a4d11085badfdfe00d34fe56bfd3c332d91cc5d3215da5
                    self.schedule.again.insert(owner);
                }
                // An owner accepted or reviewed is planned again, with the reason the plan
                // gives it now, and runs again at its place in plan order.
                // @lfy def/cli/main.lfy:compile#compile:compile:832c4ab9f9106d5819645657a00f8b70a71f5d25244ab1212be34cffa6e9bbd1
                Some(place) if set.units[place].state.is_done() => {
                    let held = &mut set.units[place];
                    held.state = UnitState::Planned;
                    held.reason = planned.reason.unwrap_or(Reason::Touched);
                    // @lfy def/cli/main.lfy:compile#compile:compile:9fb81fa1507e767993a4d11085badfdfe00d34fe56bfd3c332d91cc5d3215da5
                    self.schedule.again.insert(owner);
                }
                Some(_) => {}
            }
        }
        self.save(&target);
        self.count_done();
        // One line per unit the edits were made in, in plan order: stubbed or touched, the
        // unit edited, then the unit whose run made them and their notes.
        // @lfy def/cli/main.lfy:compile#compile:compile:186f5bf02a1197f42b15d46a728b008732f742e22b0a3360bbff586dc9f0dcab
        for owner in owners {
            let stem = self.stem(owner);
            let made: Vec<&ForeignEdit> = edits.iter().filter(|edit| edit.owner == stem).collect();
            let step = if made.iter().any(|edit| edit.kind == ForeignEditKind::Stub) { Step::Stubbed } else { Step::Touched };
            let notes: Vec<&str> = made.iter().map(|edit| edit.note.as_str()).filter(|note| !note.is_empty()).collect();
            let message =
                if notes.is_empty() { format!("by {by}") } else { format!("by {by}: {}", notes.join("; ")) };
            self.reporter.report(step, Some(&stem), &message);
        }
    }

    /// The plan made again as the compile's first plan was, with the foreign edits of every
    /// change set recorded since their owner was last accepted in place of those it was
    /// given.
    // @lfy def/cli/main.lfy:compile#compile:compile:fc8d9730ea95e71cb418a2d3c7249b91a9f7ca20d5041d67b3296632614122c8
    pub(crate) fn plan_again(&mut self) {
        let edits: Vec<ForeignEdit> =
            self.sets.values().flat_map(|set| set.recorded_since().into_iter().cloned()).collect();
        let root = self.root();
        let (program, mut plan) = self.planning.plan(&root, &edits);
        // The units the change sets accepted stay planned as they were.
        // @lfy def/cli/main.lfy:compile#compile:compile:7ddc3a3c56212d70e81bbe0dc295510b33a966489a029c6a5c3c3a0907c4b225
        keep_kept_units(&program, &mut plan, &self.sets, &self.planning.requested);
        self.replan(program, plan);
    }

    /// The binding one hosted session is opened with: what the run is run as, the unit it is
    /// for, the directory it runs in, the plan and the program the command made, and what the
    /// compiler or the verifier was handed as its input.
    // @lfy def/cli/main.lfy:compile#compile:compile:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
    #[expect(clippy::too_many_arguments, reason = "a binding is all of this")]
    pub(crate) fn binding_of(
        &self,
        role: Role,
        unit: Option<usize>,
        directory: &Path,
        request: Option<Request>,
        before: Option<Vec<Output>>,
        review: Option<ReviewRequest>,
        source_maps: Vec<SourceMap>,
    ) -> Binding {
        // @lfy def/cli/main.lfy:compile#compile:compile:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
        Binding {
            role,
            unit,
            directory: directory.to_path_buf(),
            plan: self.plan.clone(),
            program: self.program.clone(),
            request,
            before,
            review,
            source_maps,
        }
    }
}

/// The lines of the region one marker begins in a text, the marker found by its file, its
/// entity or line, and its requirement; nothing when the text holds no such marker.
// @lfy def/cli/main.lfy:compile#compile:compile:d67feda78f528338c7b27d494e8089387777160911a29fc46173c99bfa1b08fd
pub(crate) fn region_lines(text: &str, region: &Marker) -> Option<Vec<String>> {
    let lines: Vec<&str> = text.lines().collect();
    let found = generation::parse_markers(text).into_iter().find(|marker| {
        marker.file == region.file
            && marker.entity == region.entity
            && marker.requirement == region.requirement
            && (marker.entity.is_some() || marker.line == region.line)
    })?;
    let last = found.end.min(lines.len());
    let first = found.output_line.saturating_sub(1).min(last);
    Some(lines[first..last].iter().map(|line| (*line).to_string()).collect())
}

/// What one run of the compiler on a unit comes to.
// @lfy def/cli/main.lfy:compile#compile:compile:37cff27bf8d7f9a2a7c28ad0e80bf42ae1dd4d15a6e7fcf1c687af211003841c
pub(crate) enum Attempt {
    /// Accepted, with the verdict to write back.
    Verdict(Verdict),
    /// Run once more, with these problems appended to the instructions.
    Again(Vec<String>),
    /// The unit stops.
    Stopped(Stop),
}

/// What verifying a unit came to.
// @lfy def/cli/main.lfy:compile#compile:compile:e53fbd4d8f8cab911011548853cc6a5424f1956917fc2d11c49399ff0acaa3cc
pub(crate) enum Verified {
    /// No violated review and no unplaced violation, or nothing asked.
    // @lfy def/cli/main.lfy:compile#compile:compile:9854e85deefe68148920771ac99fcfeab835dc084af3382a76328d6730b88e94
    Passed,
    /// A violated review or an unplaced violation: the problems they make, and the records
    /// the review made once more is made with.
    // @lfy def/cli/main.lfy:compile#compile:compile:c3711c547c11411e55264eed71019f21008660209ec5348f9c940bbad6ddeb23
    Violated { problems: Vec<String>, next: Vec<ReviewRecord> },
    /// The compile stopped while the verifier ran.
    Stopped,
}
