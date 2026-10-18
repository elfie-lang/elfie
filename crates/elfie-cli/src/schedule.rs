// @lfy def/cli/main.lfy:main
//! The units of a compile run one at a time, in plan order: which unit runs next, what one
//! unit's run comes to from its first run to its outcome, what stops the compile or skips a
//! unit, and what the foreign edits an accepted unit made do to the units after it.

use super::*;
use crate::compiling::{append_problems, keep_kept_units};
use crate::processes::Runs;
use crate::running::{Run, Stop, files_in};

/// What a unit's first run is: the compiler, or what may stand in for it.
// @lfy def/cli/main.lfy:main#main:main:d43663ac22c59c11af52567c184b43ae8ce9a8ccec6837a4f398f5a82a41c8e5
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Stage {
    /// The compiler runs on the unit, as on any unit.
    // @lfy def/cli/main.lfy:main#main:main:d43663ac22c59c11af52567c184b43ae8ce9a8ccec6837a4f398f5a82a41c8e5
    Compiler,
    /// The unit is reworded only, so moving the markers of its reworded criteria and tests is
    /// the whole of the work and its outputs are checked without the compiler.
    // @lfy def/cli/main.lfy:main#main:main:de764e9bec0ae168fa8e14d16b3e076118e5e14f84e28ede8e6741dcaed2fdb2
    Reworded,
    /// The unit has the reason dependency, so the unit gate stands in for the compiler.
    // @lfy def/cli/main.lfy:main#main:main:f82d76ff8e23e283093211b99bd7fa4f7edfa0a9f9151cf64b7864379ceec690
    Dependency,
    /// The unit was touched, and every foreign edit made in it is a fix, so its outputs are
    /// checked as they are without the compiler.
    // @lfy def/cli/main.lfy:main#main:main:5a4cb3de145f7ebefbf1435850401e340fe94f2f917de543367904423b0c7c7d
    Fixes,
}

/// What working one unit came to.
// @lfy def/cli/main.lfy:main#main:main:2a92e13e1e56255b4cfa65e553c091ce8af723b6f9bb3156ecf05d84e8637a96
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ended {
    /// It is accepted, or reviewed, in its change set.
    Done,
    /// It stopped without being accepted.
    Stopped(Stop),
}

/// The files of a unit's output directory as they were when it started, by their path
/// relative to its directory, so that a unit that stops puts them back.
// @lfy def/cli/main.lfy:main#main:main:2a92e13e1e56255b4cfa65e553c091ce8af723b6f9bb3156ecf05d84e8637a96
pub(crate) type Snapshot = BTreeMap<String, Vec<u8>>;

/// What a unit's run of the compiler and of the verifier has come to so far, which decides
/// whether it runs once more or stops.
// @lfy def/cli/main.lfy:main#main:main:1672da5d763ee2c0c8a1a0e012ba1eb3a94200faedfbafa2b5a74254c696bfb6
#[derive(Default)]
pub(crate) struct Tries {
    /// How many times the compiler ran on the unit; a skipped first run counts as none.
    // @lfy def/cli/main.lfy:main#main:main:1672da5d763ee2c0c8a1a0e012ba1eb3a94200faedfbafa2b5a74254c696bfb6
    pub(crate) compiled: usize,
    /// Whether an outcome of the compiler was rejected, or the unit gate failed after it.
    // @lfy def/cli/main.lfy:main#main:main:f40c7cbc2f5db95ecf79b235e99ffcf4cdd0e5476ad18799f8aea789328ec588
    pub(crate) rejected: bool,
    /// Whether the verifier's report held a violated review or an unplaced violation.
    // @lfy def/cli/main.lfy:main#main:main:989f6f309c8ec4cc4f9e9682676ed8016bcfaac8a5f6ccf14d33c1bc0207a0ec
    pub(crate) violated: bool,
    /// Whether a run of the compiler failed or could not be started.
    // @lfy def/cli/main.lfy:main#main:main:dd52977c89d0dc47ea822e2276bb218642c632ccb9d02c7fcab501cb4c7b8e0e
    pub(crate) failed: bool,
    /// Whether the next run of the compiler goes on from the session of its run before it.
    // @lfy def/cli/main.lfy:main#main:main:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
    pub(crate) resume_compiler: bool,
    /// Whether the next run of the verifier goes on from the session of its run before it.
    // @lfy def/cli/main.lfy:main#main:main:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
    pub(crate) resume_verifier: bool,
}

impl Run {
    /// Every planned unit run, one at a time, in plan order, whatever its target: a unit
    /// starts only once every planned unit before it is done, skipped, or stopped. A unit
    /// that depends, directly or through other units, on one that stopped is skipped with
    /// --continue, and without it the compile stops there and no unit starts after it.
    // @lfy def/cli/main.lfy:main#main:main:de26823f65cede55489b74aa666da89098e28a63c56aff633a4c046e0170b4a1
    // @lfy def/cli/main.lfy:main#main:main:d6af6f4d3977a8dd794735d5f4d494c25c55465c8422412f7c2a41ef0f2ce483
    // @lfy def/cli/main.lfy:main#main:main:983a29cf93655951616c5c562ff1c6178966fa0f7acb433740f541446615af08
    // @lfy def/cli/main.lfy:main#main:main:b298aad200f8b11abdc3d923f5d89a20461d9597ab75a2a1ecf41d1410a4150a
    pub(crate) fn run_units(&mut self, command: &str) {
        let mut index = 0;
        // The plan is made again as units are accepted, but its order stays as it was, so
        // walking it by place visits every unit in plan order; a unit before the one just
        // worked that its foreign edits planned again is gone back to, so that no unit starts
        // while one before it is still to run.
        // @lfy def/cli/main.lfy:main#main:main:de26823f65cede55489b74aa666da89098e28a63c56aff633a4c046e0170b4a1
        // @lfy def/cli/main.lfy:main#main:main:d6af6f4d3977a8dd794735d5f4d494c25c55465c8422412f7c2a41ef0f2ce483
        // @lfy def/cli/main.lfy:main#main:main:832c4ab9f9106d5819645657a00f8b70a71f5d25244ab1212be34cffa6e9bbd1
        while index < self.plan.units.len() {
            // The compile stopped: no unit starts after that.
            // @lfy def/cli/main.lfy:main#main:main:51b22c76c1410a187c0f287178a9ecb28048b54a983754ad873fa703eb7809a3
            // @lfy def/cli/main.lfy:main#main:main:c1e8fec574c59c5e1bad63913eed21885e232f0b8264e71cda5d800113f8f466
            if self.halted {
                return;
            }
            let unit = index;
            index += 1;
            if !self.planned_units().contains(&unit) {
                continue;
            }
            let waiting = self.waiting_before(unit);
            self.run_one(unit, command);
            // @lfy def/cli/main.lfy:main#main:main:832c4ab9f9106d5819645657a00f8b70a71f5d25244ab1212be34cffa6e9bbd1
            if let Some(back) = self.waiting_before(unit).difference(&waiting).next() {
                index = *back;
            }
        }
    }

    /// The planned units before one in plan order that are still to run: neither accepted nor
    /// reviewed, and neither stopped nor skipped.
    // @lfy def/cli/main.lfy:main#main:main:d6af6f4d3977a8dd794735d5f4d494c25c55465c8422412f7c2a41ef0f2ce483
    pub(crate) fn waiting_before(&self, unit: usize) -> BTreeSet<usize> {
        self.planned_units()
            .into_iter()
            .filter(|&other| other < unit)
            .filter(|&other| !self.state_of(other).is_done())
            .filter(|other| !self.stopped.contains(other) && !self.skipped.contains(other))
            .collect()
    }

    /// One planned unit taken in its turn: verified when it is kept accepted, skipped when it
    /// depends on a unit that stopped, and worked from its first run otherwise.
    // @lfy def/cli/main.lfy:main#main:main:de26823f65cede55489b74aa666da89098e28a63c56aff633a4c046e0170b4a1
    pub(crate) fn run_one(&mut self, unit: usize, command: &str) {
        {
            let state = self.state_of(unit);
            // A unit kept accepted in a resumed change set is verified before the next unit
            // runs, as after its unit gate passed; one that stops then puts its files back as
            // they were before this run of it.
            // @lfy def/cli/main.lfy:main#main:main:95c66de9113b3ef1366369e6578cf53eb8fedb0261e03f38cf1485bd6c97bc76
            // @lfy def/cli/main.lfy:main#main:main:2a92e13e1e56255b4cfa65e553c091ce8af723b6f9bb3156ecf05d84e8637a96
            if state == UnitState::Accepted && self.verifies() {
                let directory = self.directory_of(unit);
                let snapshot = self.first_snapshot(unit, &directory);
                if let Ended::Stopped(stop) = self.verify_kept(unit, command) {
                    self.stopped_unit(unit, stop, &snapshot);
                }
                return;
            }
            if state.is_done() || self.stopped.contains(&unit) || self.skipped.contains(&unit) {
                return;
            }
        }
        // A unit that depends on one that stopped, directly or through other units, is
        // skipped: it does not run and its state stays planned.
        // @lfy def/cli/main.lfy:main#main:main:983a29cf93655951616c5c562ff1c6178966fa0f7acb433740f541446615af08
        if self.plan.units[unit]
            .dependencies
            .iter()
            .any(|dependency| self.stopped.contains(dependency) || self.skipped.contains(dependency))
        {
            self.skipped.insert(unit);
            return;
        }
        // @lfy def/cli/main.lfy:main#main:main:a9e2e9cc50ca956b7eac47a753474b23cfa890fab1725feb5ff5078999bbc30d
        let directory = self.directory_of(unit);
        let snapshot = self.first_snapshot(unit, &directory);
        if let Ended::Stopped(stop) = self.work_unit(unit, command, &directory) {
            self.stopped_unit(unit, stop, &snapshot);
        }
    }

    /// The files of a unit's output directory as they were before its first run in the
    /// compile: taken in its directory the first time it runs, and the same every time after,
    /// however often it runs again.
    // @lfy def/cli/main.lfy:main#main:main:2a92e13e1e56255b4cfa65e553c091ce8af723b6f9bb3156ecf05d84e8637a96
    pub(crate) fn first_snapshot(&mut self, unit: usize, directory: &Path) -> Snapshot {
        if let Some(taken) = self.snapshots.get(&unit) {
            return taken.clone();
        }
        let taken = snapshot_of(self.target_of_unit(unit), directory);
        self.snapshots.insert(unit, taken.clone());
        taken
    }

    /// One unit worked from its first run to its outcome, in its change set's copy: the
    /// compiler, the verifier, and the target's commands all run for it there.
    ///
    /// Its first run may skip the compiler, as a reworded-only, a dependency-only, or a
    /// touched-fix unit's may; a skipped run counts as no run of the compiler in any count that
    /// decides whether the unit is run once more or stops, and anything that goes wrong with it
    /// sends the unit to the compiler as its first run.
    // @lfy def/cli/main.lfy:main#main:main:a9e2e9cc50ca956b7eac47a753474b23cfa890fab1725feb5ff5078999bbc30d
    // @lfy def/cli/main.lfy:main#main:main:3794ecd87cdab206cb18cf520d802e324546a2124e9c3ab4f934dc88f93dbb58
    // @lfy def/cli/main.lfy:main#main:main:1672da5d763ee2c0c8a1a0e012ba1eb3a94200faedfbafa2b5a74254c696bfb6
    pub(crate) fn work_unit(&mut self, unit: usize, command: &str, directory: &Path) -> Ended {
        let stem = self.stem(unit);
        let reason = self.plan.units[unit].reason.map_or_else(|| "up to date".to_string(), |r| r.as_str().to_string());
        // What may stand in for the compiler is decided before any marker of the unit's
        // outputs is rewritten: the outputs on disk must be exactly the ones the last
        // acceptance recorded.
        // @lfy def/cli/main.lfy:main#main:main:de764e9bec0ae168fa8e14d16b3e076118e5e14f84e28ede8e6741dcaed2fdb2
        // @lfy def/cli/main.lfy:main#main:main:f82d76ff8e23e283093211b99bd7fa4f7edfa0a9f9151cf64b7864379ceec690
        let stand = self.outputs_stand(unit, directory);
        self.reporter.report(Step::Requesting, Some(&stem), &reason);
        // Before each request, the markers of the unit's reworded criteria and tests name the
        // ids they have now.
        // @lfy def/cli/main.lfy:main#main:main:7b0497097e610d4da1457726876ccfb45f7a41d3d5170bf29a1986426750c122
        self.reword_outputs(unit, directory);
        // The request is given, as the unit's foreign edits, those recorded since it was last
        // accepted in its change set, in their order.
        // @lfy def/cli/main.lfy:main#main:main:f210a94b1b3335dbbbe49bccc300d317f4cdb0a7e7281664426db68726bc71f6
        let edits: Vec<ForeignEdit> =
            self.set_of(unit).map(|set| set.edits_since(&stem).into_iter().cloned().collect()).unwrap_or_default();
        // @lfy def/cli/main.lfy:main#main:main:3794ecd87cdab206cb18cf520d802e324546a2124e9c3ab4f934dc88f93dbb58
        let (mut request, accepted_source) = self.request_of(unit, directory, true, &edits);
        let mut stage = self.first_stage(unit, &request, stand, accepted_source);
        let runs = Runs::new(self.agent.clone(), self.running.clone());
        let mut tries = Tries::default();
        // What the unit's first review carries: what it was last reviewed as, in its
        // directory.
        // @lfy def/cli/main.lfy:main#main:main:974936f3c3946711196e8a624f7b7a1eef10cd5508bc5dc018f39ef22112a2df
        let mut records = self.records_of(unit, directory);
        // The files of the output directory just before the first run of the compiler.
        // @lfy def/cli/main.lfy:main#main:main:868619066ec30b207dd51ea0ca22774e291fcf2bfb0b448ee749c225324e5fe9
        let mut before: Option<Vec<Output>> = None;
        loop {
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
                // @lfy def/cli/main.lfy:main#main:main:1535e1a49cabe0f48836a4331c9b37e4b542f78aa3bfec5c56622eebf9cd3707
                // @lfy def/cli/main.lfy:main#main:main:ce28b2a5274a7561b5410f710bf240bf388a32b2bcbc463520600896aeb19e78
                Stage::Dependency => {
                    let maps = self.maps_in(unit, directory);
                    if let Err(problem) = self.unit_gate(unit, &maps, directory) {
                        self.reporter.problems(std::slice::from_ref(&problem));
                        stage = Stage::Compiler;
                        continue;
                    }
                    let now = files_in(self.target_of_unit(unit), directory);
                    let verdict = self.verdict_of(unit, &request, directory, Some(&now), &BTreeMap::new());
                    if !verdict.accepted {
                        self.report_rejection(unit, &verdict.problems);
                        // @lfy def/cli/main.lfy:main#main:main:ce28b2a5274a7561b5410f710bf240bf388a32b2bcbc463520600896aeb19e78
                        self.reporter.report(Step::Building, Some(&stem), "the unit was rejected after its gate passed");
                        stage = Stage::Compiler;
                        continue;
                    }
                    // The unit is accepted on its unit gate: its outputs are written back, no
                    // verifier runs on it, its maps are recorded, no review record file
                    // changes, and it is done, reviewed.
                    // @lfy def/cli/main.lfy:main#main:main:166f8e18960857f1947c51e52ef31c65f43c90ee02cf96a66d0306bae7adb34f
                    if !self.write_back(unit, &verdict, directory) {
                        return Ended::Stopped(Stop::Failed);
                    }
                    self.reporter.report(Step::Accepted, Some(&stem), &format!("{} outputs", verdict.outputs.len()));
                    self.accept_in_set(unit, verdict.foreign_edits.clone());
                    // @lfy def/cli/main.lfy:main#main:main:166f8e18960857f1947c51e52ef31c65f43c90ee02cf96a66d0306bae7adb34f
                    self.record_maps(unit, &verdict.source_maps, directory, false);
                    self.set_state(unit, UnitState::Reviewed);
                    return Ended::Done;
                }
                // The outputs are checked as they are, with every file of the listing as it is
                // now as the files before the run, and with no notes.
                // @lfy def/cli/main.lfy:main#main:main:de764e9bec0ae168fa8e14d16b3e076118e5e14f84e28ede8e6741dcaed2fdb2
                // @lfy def/cli/main.lfy:main#main:main:5a4cb3de145f7ebefbf1435850401e340fe94f2f917de543367904423b0c7c7d
                Stage::Reworded | Stage::Fixes => {
                    let now = files_in(self.target_of_unit(unit), directory);
                    let verdict = self.verdict_of(unit, &request, directory, Some(&now), &BTreeMap::new());
                    if !verdict.accepted {
                        // The problems are printed and the compiler runs on the unit as its
                        // first run; the rejection counts toward no retry.
                        // @lfy def/cli/main.lfy:main#main:main:4f28bce06fb2870b28cb51c505e5fc1c33b4a5834d9c1680e3ff4d797d43ec28
                        // @lfy def/cli/main.lfy:main#main:main:ba06a4655e253030d84c2c80b9e79c771fd19cd90fe47216e277cabfc5e530d3
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
            // @lfy def/cli/main.lfy:main#main:main:87ae3376d10498cdfb807849c1ba3d8243944e7c065752aafa1a959603280213
            // @lfy def/cli/main.lfy:main#main:main:5bdebf278eae36f6a91cb9b544d6b8e89fef337d885f3d2dbcdc7cde773503ec
            // @lfy def/cli/main.lfy:main#main:main:e3d4906500c6a0dc5bca8cb0c0f72d9d30c6afb8e30feb4410b663fb8a31b5f5
            if !self.write_back(unit, &verdict, directory) {
                return Ended::Stopped(Stop::Failed);
            }
            if let Err(problem) = self.unit_gate(unit, &verdict.source_maps, directory) {
                // @lfy def/cli/main.lfy:main#main:main:c863a527b6e120083837fbd590e228d380afa98faa81f3564fd62d4a4bad94b4
                let problems = vec![problem];
                self.report_rejection(unit, &problems);
                // A first run that skipped the compiler sends the unit to the compiler as its
                // first run, counting toward no retry.
                // @lfy def/cli/main.lfy:main#main:main:4f28bce06fb2870b28cb51c505e5fc1c33b4a5834d9c1680e3ff4d797d43ec28
                // @lfy def/cli/main.lfy:main#main:main:ba06a4655e253030d84c2c80b9e79c771fd19cd90fe47216e277cabfc5e530d3
                if !compiled {
                    if stage == Stage::Fixes {
                        append_problems(&mut request, &problems);
                    }
                    stage = Stage::Compiler;
                    continue;
                }
                // The first time, the unit is run once more with the problems appended; the
                // second time, it is rejected after its retries.
                // @lfy def/cli/main.lfy:main#main:main:f40c7cbc2f5db95ecf79b235e99ffcf4cdd0e5476ad18799f8aea789328ec588
                // @lfy def/cli/main.lfy:main#main:main:a03e4af397a908b8bd05bee7d8a51f0a7f6a6441fdf159eaec23abb14bcae550
                if tries.rejected {
                    return Ended::Stopped(Stop::Rejected);
                }
                tries.rejected = true;
                tries.resume_compiler = true;
                append_problems(&mut request, &problems);
                continue;
            }
            self.reporter.report(Step::Accepted, Some(&stem), &format!("{} outputs", verdict.outputs.len()));
            // Accepted and through its unit gate: its state becomes accepted and the foreign
            // edits it made are recorded in its change set.
            // @lfy def/cli/main.lfy:main#main:main:1effb63cfadc95b40e6ebdc615397f053aaa2c7015afe87022399dbfda5f1f7a
            self.accept_in_set(unit, verdict.foreign_edits.clone());
            // No verifier, or --no-verify: the maps are recorded in the unit's directory and
            // the unit is done, accepted.
            // @lfy def/cli/main.lfy:main#main:main:6b0a3b7ef1ed03adae1066e2bb21b9ada933d5808f1039dce913c2635f24308d
            // @lfy def/cli/main.lfy:main#main:main:08a03e03ed21c2434de1da9166f40bcab6b1b4d3ce579940f544901e26051a6c
            if !self.verifies() {
                self.record_maps(unit, &verdict.source_maps, directory, false);
                return Ended::Done;
            }
            // The unit is verified.
            // @lfy def/cli/main.lfy:main#main:main:da396078b042c11950a5c194c204293f329886e37ab0ea41c0a36030c64d3d56
            // @lfy def/cli/main.lfy:main#main:main:5f08ae8ce4c83f4e1b7cc28af35e05f37af1782031238c3c21ed4a69cae58b08
            match self.verify_unit(unit, command, directory, &verdict.source_maps, &records, &runs, tries.resume_verifier) {
                Verified::Passed => {
                    // @lfy def/cli/main.lfy:main#main:main:a9a35de62ed7e8af9e763d1d17f62b520074b0ff62f6d127c8a757f06b3d2654
                    self.record_maps(unit, &verdict.source_maps, directory, true);
                    self.set_state(unit, UnitState::Reviewed);
                    return Ended::Done;
                }
                Verified::Stopped => return Ended::Stopped(Stop::Failed),
                Verified::Violated { problems, next } => {
                    // The unit's state is planned while it runs again.
                    // @lfy def/cli/main.lfy:main#main:main:836ff964045c3fdf6d9f590423c540dcc1814918a94ca6164f5840c078d87342
                    self.set_state(unit, UnitState::Planned);
                    // The second such report rejects the unit after its retries, and its maps
                    // are not recorded.
                    // @lfy def/cli/main.lfy:main#main:main:a8d0e0a443a2adbf0143baf22ffaa2b0ec6dc8b39fb3efe5c9ec8d26eee79b87
                    if tries.violated {
                        return Ended::Stopped(Stop::Rejected);
                    }
                    // The first runs the unit once more with the problems appended, even when it
                    // was already run once more for a rejected outcome: the compiler's first
                    // run when it has not run on the unit yet, and a resuming run when it has.
                    // @lfy def/cli/main.lfy:main#main:main:989f6f309c8ec4cc4f9e9682676ed8016bcfaac8a5f6ccf14d33c1bc0207a0ec
                    // @lfy def/cli/main.lfy:main#main:main:ec0e37d5ec1e376b65aa90a57f20654ed0153dcd1c1cb0899bf5f1b988e4c2ce
                    // The verifier's run once more resumes only after a resuming run of the
                    // compiler, which there is only when the compiler ran on the unit before.
                    // @lfy def/cli/main.lfy:main#main:main:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
                    // @lfy def/cli/main.lfy:main#main:main:631eb66f63680435474026b050f06277c5eee4e5b444e767c71386bdbb3c3118
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
    /// request carries no foreign edit, its outputs stand, and its previous text is its source
    /// at its last accepted generation. A unit with the reason dependency has the unit gate run
    /// instead, while the target has a line for build and for test, its request carries no
    /// foreign edit, and its outputs stand. A touched unit whose foreign edits are all fixes is
    /// checked as it is. Every other unit, one touched by a stub among them, goes to the
    /// compiler.
    // @lfy def/cli/main.lfy:main#main:main:de764e9bec0ae168fa8e14d16b3e076118e5e14f84e28ede8e6741dcaed2fdb2
    // @lfy def/cli/main.lfy:main#main:main:10b9ec9d79a2f879f1f76e724fef7226300dd6b087d5accf4ca2f46f5102e90b
    // @lfy def/cli/main.lfy:main#main:main:f82d76ff8e23e283093211b99bd7fa4f7edfa0a9f9151cf64b7864379ceec690
    // @lfy def/cli/main.lfy:main#main:main:343acafbb0e354bca79a21aac598241ee5886911dba44bf7b3766bec69054122
    // @lfy def/cli/main.lfy:main#main:main:5a4cb3de145f7ebefbf1435850401e340fe94f2f917de543367904423b0c7c7d
    // @lfy def/cli/main.lfy:main#main:main:8214a980d0dc5f7d2ad165f456c7096d6e4ef89966519c6a783ecc99a04969fd
    // @lfy def/cli/main.lfy:main#main:main:d43663ac22c59c11af52567c184b43ae8ce9a8ccec6837a4f398f5a82a41c8e5
    // @lfy def/cli/main.lfy:main#main:main:92dda467736030409d0aa3d11b09f0e0dfe83df48a8098c9b062fb817c2ed45e
    pub(crate) fn first_stage(&self, unit: usize, request: &Request, stand: bool, accepted_source: bool) -> Stage {
        let reason = self.plan.units[unit].reason;
        let edits = &request.foreign_edits;
        // @lfy def/cli/main.lfy:main#main:main:5a4cb3de145f7ebefbf1435850401e340fe94f2f917de543367904423b0c7c7d
        if reason == Some(Reason::Touched)
            && !edits.is_empty()
            && edits.iter().all(|edit| edit.kind == ForeignEditKind::Fix)
        {
            return Stage::Fixes;
        }
        // An output that is no longer what was accepted, or a foreign edit in it, leaves the
        // compiler to do.
        // @lfy def/cli/main.lfy:main#main:main:92dda467736030409d0aa3d11b09f0e0dfe83df48a8098c9b062fb817c2ed45e
        if !stand || !edits.is_empty() {
            return Stage::Compiler;
        }
        // @lfy def/cli/main.lfy:main#main:main:de764e9bec0ae168fa8e14d16b3e076118e5e14f84e28ede8e6741dcaed2fdb2
        if request.reworded_only && self.verifies() && accepted_source {
            return Stage::Reworded;
        }
        // @lfy def/cli/main.lfy:main#main:main:f82d76ff8e23e283093211b99bd7fa4f7edfa0a9f9151cf64b7864379ceec690
        if reason == Some(Reason::Dependency) && self.builds_and_tests(unit) {
            return Stage::Dependency;
        }
        // @lfy def/cli/main.lfy:main#main:main:d43663ac22c59c11af52567c184b43ae8ce9a8ccec6837a4f398f5a82a41c8e5
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
    // @lfy def/cli/main.lfy:main#main:main:3794ecd87cdab206cb18cf520d802e324546a2124e9c3ab4f934dc88f93dbb58
    // @lfy def/cli/main.lfy:main#main:main:6d22af1c9b1faa80650ee222bf29ec59916fad170472c864b3ea67c7206c69c9
    // @lfy def/cli/main.lfy:main#main:main:91ffd2f6eafdd00f74f9012574d3c7ae2bd6569a8b89e66b9bfb6b117a6c8488
    // @lfy def/cli/main.lfy:main#main:main:f8eef3f6a490308c065b3e1e9de537e4ca8b958f1a91454d15e2678950511b8b
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
        // The unit's files before its first run of the compiler, as they read in its directory
        // just then.
        // @lfy def/cli/main.lfy:main#main:main:868619066ec30b207dd51ea0ca22774e291fcf2bfb0b448ee749c225324e5fe9
        let earlier = before.get_or_insert_with(|| files_in(self.target_of_unit(unit), directory)).clone();
        // The run has the role compiler, the unit, the request its input was made from, the
        // files before its first run, no review, and no source maps.
        // @lfy def/cli/main.lfy:main#main:main:6d22af1c9b1faa80650ee222bf29ec59916fad170472c864b3ea67c7206c69c9
        let binding = self.binding_of(Role::Compiler, Some(unit), directory, Some(request.clone()), Some(earlier.clone()), None, Vec::new());
        // @lfy def/cli/main.lfy:main#main:main:3794ecd87cdab206cb18cf520d802e324546a2124e9c3ab4f934dc88f93dbb58
        let exit = runs.run(command, directory, &stem, &request.instructions, binding, tries.resume_compiler);
        // An agent server that could not be hosted runs nothing more: the compile stops, and
        // the code is failure.
        // @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
        if self.agent.server.failed() {
            self.reporter.report(Step::Failed, Some(&stem), exit.stderr.trim());
            self.reporter.problems(&[exit.stderr.trim().to_string()]);
            complain(self.style.err, None, exit.stderr.trim());
            self.worsen(ExitCode::Failure);
            self.halted = true;
            return Attempt::Stopped(Stop::Failed);
        }
        let outcome = if exit.code == -1 {
            // The code is -1 because the command could not be started or was ended: the
            // outcome is failed.
            // @lfy def/cli/main.lfy:main#main:main:dd52977c89d0dc47ea822e2276bb218642c632ccb9d02c7fcab501cb4c7b8e0e
            Outcome {
                kind: OutcomeKind::Failed,
                message: format!("the compiler command could not be run: {}", exit.stderr.trim()),
                verdict: None,
            }
        } else {
            // Process.Exit.stdout is the report; each of its lines reached the log as it was
            // shown, so nothing more of it is appended.
            // @lfy def/cli/main.lfy:main#main:main:3082965661f2495d1e5e9d8a2177fc2ca759320db1b83c7e7bc0605b9be65f3f
            // @lfy def/cli/main.lfy:main#main:main:91ffd2f6eafdd00f74f9012574d3c7ae2bd6569a8b89e66b9bfb6b117a6c8488
            let verdict = self.verdict_of(unit, request, directory, Some(&earlier), &exit.record.touches);
            // @lfy def/cli/main.lfy:main#main:main:f8eef3f6a490308c065b3e1e9de537e4ca8b958f1a91454d15e2678950511b8b
            generation::outcome_of(&exit.stdout, Some(verdict), exit.record.submission.as_ref())
        };
        match outcome.kind {
            OutcomeKind::Accepted => match outcome.verdict {
                Some(verdict) => Attempt::Verdict(verdict),
                None => Attempt::Stopped(Stop::Failed),
            },
            // The problems are printed; the first time the unit is run once more with them,
            // and the second time it is rejected after its retries.
            // @lfy def/cli/main.lfy:main#main:main:c863a527b6e120083837fbd590e228d380afa98faa81f3564fd62d4a4bad94b4
            // @lfy def/cli/main.lfy:main#main:main:f40c7cbc2f5db95ecf79b235e99ffcf4cdd0e5476ad18799f8aea789328ec588
            // @lfy def/cli/main.lfy:main#main:main:a03e4af397a908b8bd05bee7d8a51f0a7f6a6441fdf159eaec23abb14bcae550
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
            // @lfy def/cli/main.lfy:main#main:main:dde649bfa6713f34a01d0dc5cd8d20ecfdd75e2922f273e2ab7980fe955ee8cb
            // @lfy def/cli/main.lfy:main#main:main:74e6415c7697aa2dadc0187f18f8dd99e61e0fa3c25ecb74732fc6daf9edd323
            // @lfy def/cli/main.lfy:main#main:main:758fa1871b09f337b51e44d8a8f6104962caf305712f1929d105dc578aa66787
            OutcomeKind::Blocked => {
                self.reporter.report(Step::Blocked, Some(&stem), &outcome.message);
                self.reporter.reason(&outcome.message);
                let note = format!("# The compiler is blocked on the unit {stem}\n\n{}\n", outcome.message);
                self.write_note(&stem, "blocked.md", &note);
                Attempt::Stopped(Stop::Blocked)
            }
            // The question is printed and written, and the unit stops with a question.
            // @lfy def/cli/main.lfy:main#main:main:2c9713a6d34075ba2ffe7490ad3a4f06a37c84997bec06ae56d74bec93a1522f
            // @lfy def/cli/main.lfy:main#main:main:28f06cfbbc544a98ab6665e6d25f69be54ee58f259107388cbd7163287389e1a
            // @lfy def/cli/main.lfy:main#main:main:c23e2d99fb15c650e2042ab1842fb2dbcc75b210acde889bbdc12eb9f947e652
            OutcomeKind::Clarification => {
                self.reporter.report(Step::Clarification, Some(&stem), &outcome.message);
                self.reporter.reason(&outcome.message);
                let note = format!(
                    "# The compiler asked about the unit {stem}\n\n{}\n\n{ANSWER_HEADING}\n\n<!-- Answer by editing the definitions, or write the answer below this line; the next compile of this unit appends it to the instructions. -->\n",
                    outcome.message
                );
                self.write_note(&stem, "question.md", &note);
                Attempt::Stopped(Stop::Question)
            }
            // The first time the unit is run once more, as no resuming run; the second time it
            // stops failed.
            // @lfy def/cli/main.lfy:main#main:main:dd52977c89d0dc47ea822e2276bb218642c632ccb9d02c7fcab501cb4c7b8e0e
            // @lfy def/cli/main.lfy:main#main:main:3ac90f9999308a6d63417d712d756867c10bc89fc52537afa640a0e82a6f5ba2
            OutcomeKind::Failed => {
                self.reporter.report(Step::Failed, Some(&stem), &outcome.message);
                self.reporter.problems(std::slice::from_ref(&outcome.message));
                if exit.code == -1 {
                    self.worsen(ExitCode::Failure);
                }
                if tries.failed {
                    return Attempt::Stopped(Stop::Failed);
                }
                tries.failed = true;
                // @lfy def/cli/main.lfy:main#main:main:631eb66f63680435474026b050f06277c5eee4e5b444e767c71386bdbb3c3118
                tries.resume_compiler = false;
                Attempt::Again(Vec::new())
            }
        }
    }

    /// A unit kept accepted in a resumed change set verified before the next unit runs, as
    /// after its unit gate passed, with the source maps `accept` derives for its outputs as
    /// they are. A unit whose outputs `accept` no longer accepts runs again instead.
    // @lfy def/cli/main.lfy:main#main:main:95c66de9113b3ef1366369e6578cf53eb8fedb0261e03f38cf1485bd6c97bc76
    pub(crate) fn verify_kept(&mut self, unit: usize, command: &str) -> Ended {
        let directory = self.directory_of(unit);
        let stem = self.stem(unit);
        let edits: Vec<ForeignEdit> =
            self.set_of(unit).map(|set| set.edits_since(&stem).into_iter().cloned().collect()).unwrap_or_default();
        let (request, _) = self.request_of(unit, &directory, true, &edits);
        let now = files_in(self.target_of_unit(unit), &directory);
        let verdict = self.verdict_of(unit, &request, &directory, Some(&now), &BTreeMap::new());
        if !verdict.accepted {
            self.set_state(unit, UnitState::Planned);
            return self.work_unit(unit, command, &directory);
        }
        let runs = Runs::new(self.agent.clone(), self.running.clone());
        let records = self.records_of(unit, &directory);
        match self.verify_unit(unit, command, &directory, &verdict.source_maps, &records, &runs, false) {
            Verified::Passed => {
                self.record_maps(unit, &verdict.source_maps, &directory, true);
                self.set_state(unit, UnitState::Reviewed);
                Ended::Done
            }
            Verified::Stopped => Ended::Stopped(Stop::Failed),
            // A violated review runs the unit again, as the first run of the compiler on it.
            // @lfy def/cli/main.lfy:main#main:main:ec0e37d5ec1e376b65aa90a57f20654ed0153dcd1c1cb0899bf5f1b988e4c2ce
            Verified::Violated { .. } => {
                self.set_state(unit, UnitState::Planned);
                self.work_unit(unit, command, &directory)
            }
        }
    }

    /// A unit that stopped without being accepted: rejected after its retries, blocked, with
    /// a question, or failed a second time.
    ///
    /// Every one of its change set's own files under the target's output directory is put
    /// back as it was before the unit's first run in the compile, and every such file added
    /// since is removed; what those files held before they were put back is written at their
    /// paths relative to the copy under `elfie-requests/<name>.rejected` under the root. A
    /// rejected unit's state is rejected, and every other's stays planned. Without --continue
    /// the compile stops; with it, every unit that depends on this one is skipped.
    // @lfy def/cli/main.lfy:main#main:main:2a92e13e1e56255b4cfa65e553c091ce8af723b6f9bb3156ecf05d84e8637a96
    // @lfy def/cli/main.lfy:main#main:main:0954f463a673ef9da817b979310906a5a5009931bd64cc4941c7822e82a8d017
    // @lfy def/cli/main.lfy:main#main:main:70167f02b1744682fa98c82bc866d02abc4a28f12d8b3ca9758aa2d5e669641d
    // @lfy def/cli/main.lfy:main#main:main:c1e8fec574c59c5e1bad63913eed21885e232f0b8264e71cda5d800113f8f466
    pub(crate) fn stopped_unit(&mut self, unit: usize, stop: Stop, snapshot: &Snapshot) {
        match stop {
            Stop::Rejected => self.rejected += 1,
            Stop::Blocked | Stop::Question => self.blocked += 1,
            Stop::Failed => self.failed += 1,
        }
        // @lfy def/cli/main.lfy:main#main:main:3d1e2128231c0dd4a623216ed6845b9bfdce55b3a624742046eb528f6cff68dd
        self.worsen(ExitCode::Problems);
        // @lfy def/cli/main.lfy:main#main:main:2a92e13e1e56255b4cfa65e553c091ce8af723b6f9bb3156ecf05d84e8637a96
        if self.set_of(unit).is_some() {
            let directory = self.directory_of(unit);
            self.put_back(unit, &directory, snapshot);
        }
        // @lfy def/cli/main.lfy:main#main:main:0954f463a673ef9da817b979310906a5a5009931bd64cc4941c7822e82a8d017
        // @lfy def/cli/main.lfy:main#main:main:70167f02b1744682fa98c82bc866d02abc4a28f12d8b3ca9758aa2d5e669641d
        let state = if stop == Stop::Rejected { UnitState::Rejected } else { UnitState::Planned };
        self.set_state(unit, state);
        self.stopped.insert(unit);
        // Without --continue the compile stops: no unit of any target runs after it, and no
        // final gate, global review, or merge runs.
        // @lfy def/cli/main.lfy:main#main:main:c1e8fec574c59c5e1bad63913eed21885e232f0b8264e71cda5d800113f8f466
        if !self.continuing {
            self.halted = true;
            // Every run still going is stopped, with every process of its group.
            // @lfy def/cli/main.lfy:main#main:main:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
            self.running.stop();
        }
    }

    /// The files of a unit's output directory in its copy put back as the snapshot taken
    /// before its first run holds them, files added since removed, and what each file put
    /// back held written under `elfie-requests/<name>.rejected` under the root.
    // @lfy def/cli/main.lfy:main#main:main:2a92e13e1e56255b4cfa65e553c091ce8af723b6f9bb3156ecf05d84e8637a96
    pub(crate) fn put_back(&mut self, unit: usize, directory: &Path, snapshot: &Snapshot) {
        let target = self.target_of_unit(unit).clone();
        let kept = self.requests_directory().join(format!("{}.rejected", request_name(&self.stem(unit))));
        let mut found = Vec::new();
        walk_all(&directory.join(&target.output_directory), &mut found);
        let now: BTreeMap<String, Vec<u8>> = found
            .iter()
            .map(|path| relative(directory, path))
            .filter(|path| self.is_own_file(unit, path))
            .filter_map(|path| fs::read(directory.join(&path)).ok().map(|held| (path, held)))
            .collect();
        // What each file held before it was put back, written under the root.
        // @lfy def/cli/main.lfy:main#main:main:2a92e13e1e56255b4cfa65e553c091ce8af723b6f9bb3156ecf05d84e8637a96
        for (path, held) in &now {
            if snapshot.get(path) == Some(held) {
                continue;
            }
            let to = kept.join(path);
            // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
            let written = to.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(&to, held));
            if let Err(error) = written {
                complain(self.style.err, Some(&to.display().to_string()), &error.to_string());
            }
            // A file added since the unit's first run is removed.
            // @lfy def/cli/main.lfy:main#main:main:2a92e13e1e56255b4cfa65e553c091ce8af723b6f9bb3156ecf05d84e8637a96
            if !snapshot.contains_key(path) {
                let _ = fs::remove_file(directory.join(path));
            }
        }
        // Every file is put back as it was.
        // @lfy def/cli/main.lfy:main#main:main:2a92e13e1e56255b4cfa65e553c091ce8af723b6f9bb3156ecf05d84e8637a96
        for (path, held) in snapshot {
            if now.get(path) == Some(held) {
                continue;
            }
            let to = directory.join(path);
            let written = to.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(&to, held));
            if let Err(error) = written {
                complain(self.style.err, Some(path), &error.to_string());
                self.worsen(ExitCode::Failure);
            }
        }
    }

    /// A unit accepted and through its unit gate recorded in its change set: its state
    /// becomes accepted with the hash and requirements it has now, and each foreign edit of
    /// its verdict is appended to the change set's, in order, its count of edits becoming
    /// theirs. The change set is written at once, and the foreign edits then do what they do
    /// to the units after it.
    // @lfy def/cli/main.lfy:main#main:main:1effb63cfadc95b40e6ebdc615397f053aaa2c7015afe87022399dbfda5f1f7a
    // @lfy def/cli/main.lfy:main#main:main:3d654e560f2e1144270b416e60c71b73f41429d32f691bba2f8f6fab0038bd7a
    pub(crate) fn accept_in_set(&mut self, unit: usize, edits: Vec<ForeignEdit>) {
        let stem = self.stem(unit);
        let target = self.target_of_unit(unit).identifier.clone();
        let hash = generation::source_hash(&self.plan.units[unit].text);
        let requirements = generation::source_hash(&requirement_ids_of(&self.program, &self.plan.units[unit]).join("\n"));
        let Some(set) = self.sets.get_mut(&target) else {
            self.states.insert(unit, UnitState::Accepted);
            self.count_done();
            return;
        };
        let Some(place) = set.units.iter().position(|held| held.stem == stem) else { return };
        // @lfy def/cli/main.lfy:main#main:main:1effb63cfadc95b40e6ebdc615397f053aaa2c7015afe87022399dbfda5f1f7a
        set.accept(place, &hash, &requirements, edits.clone());
        self.save(&target);
        self.count_done();
        if !edits.is_empty() {
            self.foreign_edits_recorded(unit, &edits);
        }
    }

    /// The foreign edits of an accepted unit recorded: the plan is made again as the first
    /// plan was, with the foreign edits recorded since their owner was last accepted in place
    /// of those it was given, so its order is the same. Each owner not yet in the change set
    /// joins it at its place in plan order with the reason touched; an owner already accepted
    /// or reviewed is planned again with the reason the plan gives it. One progress line then
    /// follows for each unit the edits were made in, in plan order: stubbed when one of the
    /// edits made in it is a stub, and touched otherwise.
    // @lfy def/cli/main.lfy:main#main:main:fc8d9730ea95e71cb418a2d3c7249b91a9f7ca20d5041d67b3296632614122c8
    // @lfy def/cli/main.lfy:main#main:main:eeb6eb51ce2fa61105a263d0328d2874ebfc0bc3ced175501ee78c99422ed3ec
    // @lfy def/cli/main.lfy:main#main:main:832c4ab9f9106d5819645657a00f8b70a71f5d25244ab1212be34cffa6e9bbd1
    // @lfy def/cli/main.lfy:main#main:main:186f5bf02a1197f42b15d46a728b008732f742e22b0a3360bbff586dc9f0dcab
    pub(crate) fn foreign_edits_recorded(&mut self, unit: usize, edits: &[ForeignEdit]) {
        let by = self.stem(unit);
        let target = self.target_of_unit(unit).identifier.clone();
        // @lfy def/cli/main.lfy:main#main:main:fc8d9730ea95e71cb418a2d3c7249b91a9f7ca20d5041d67b3296632614122c8
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
                // @lfy def/cli/main.lfy:main#main:main:eeb6eb51ce2fa61105a263d0328d2874ebfc0bc3ced175501ee78c99422ed3ec
                None => {
                    let at = set
                        .units
                        .iter()
                        .position(|held| {
                            order.iter().position(|stem| *stem == held.stem) > order.iter().position(|stem| *stem == planned.stem)
                        })
                        .unwrap_or(set.units.len());
                    set.units.insert(at, ChangeSetUnit::planned(&planned.stem, Reason::Touched, "", ""));
                }
                // An owner accepted or reviewed is planned again, with the reason the plan
                // gives it now, and runs again at its place in plan order.
                // @lfy def/cli/main.lfy:main#main:main:832c4ab9f9106d5819645657a00f8b70a71f5d25244ab1212be34cffa6e9bbd1
                Some(place) if set.units[place].state.is_done() => {
                    let held = &mut set.units[place];
                    held.state = UnitState::Planned;
                    held.reason = planned.reason.unwrap_or(Reason::Touched);
                }
                Some(_) => {}
            }
        }
        self.save(&target);
        self.count_done();
        // One line per unit the edits were made in, in plan order: stubbed or touched, the
        // unit edited, then the unit whose run made them and their notes.
        // @lfy def/cli/main.lfy:main#main:main:186f5bf02a1197f42b15d46a728b008732f742e22b0a3360bbff586dc9f0dcab
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
    // @lfy def/cli/main.lfy:main#main:main:fc8d9730ea95e71cb418a2d3c7249b91a9f7ca20d5041d67b3296632614122c8
    pub(crate) fn plan_again(&mut self) {
        let edits: Vec<ForeignEdit> =
            self.sets.values().flat_map(|set| set.recorded_since().into_iter().cloned()).collect();
        let root = self.root();
        let (program, mut plan) = self.planning.plan(&root, &edits);
        // The units the change sets accepted stay planned as they were.
        // @lfy def/cli/main.lfy:main#main:main:7ddc3a3c56212d70e81bbe0dc295510b33a966489a029c6a5c3c3a0907c4b225
        keep_kept_units(&program, &mut plan, &self.sets, &self.planning.requested);
        self.replan(program, plan);
    }

    /// The binding one hosted session is opened with: what the run is run as, the unit it is
    /// for, the directory it runs in, the plan and the program the command made, and what the
    /// compiler or the verifier was handed as its input.
    // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
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
        // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
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

/// What one run of the compiler on a unit comes to.
// @lfy def/cli/main.lfy:main#main:main:f8eef3f6a490308c065b3e1e9de537e4ca8b958f1a91454d15e2678950511b8b
pub(crate) enum Attempt {
    /// Accepted, with the verdict to write back.
    Verdict(Verdict),
    /// Run once more, with these problems appended to the instructions.
    Again(Vec<String>),
    /// The unit stops.
    Stopped(Stop),
}

/// What verifying a unit came to.
// @lfy def/cli/main.lfy:main#main:main:e53fbd4d8f8cab911011548853cc6a5424f1956917fc2d11c49399ff0acaa3cc
pub(crate) enum Verified {
    /// No violated review and no unplaced violation, or nothing asked.
    // @lfy def/cli/main.lfy:main#main:main:a9a35de62ed7e8af9e763d1d17f62b520074b0ff62f6d127c8a757f06b3d2654
    Passed,
    /// A violated review or an unplaced violation: the problems they make, and the records
    /// the review made once more is made with.
    // @lfy def/cli/main.lfy:main#main:main:6e067459146b4d2bf024656cdb23445dfe967a1375be4a87f72a515e611ffef4
    Violated { problems: Vec<String>, next: Vec<ReviewRecord> },
    /// The compile stopped while the verifier ran.
    Stopped,
}

/// The files of a target's output directory in a directory, as they are, among the change
/// set's own files.
// @lfy def/cli/main.lfy:main#main:main:2a92e13e1e56255b4cfa65e553c091ce8af723b6f9bb3156ecf05d84e8637a96
pub(crate) fn snapshot_of(target: &Target, directory: &Path) -> Snapshot {
    let mut found = Vec::new();
    walk_all(&directory.join(&target.output_directory), &mut found);
    found
        .iter()
        .map(|path| relative(directory, path))
        .filter(|path| crate::copies::is_own(path, target))
        .filter_map(|path| fs::read(directory.join(&path)).ok().map(|held| (path, held)))
        .collect()
}
