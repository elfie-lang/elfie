// @lfy def/cli/main.lfy:main
//! Reviewing what was compiled: the review request of one batch, the verifier runs that
//! answer it, the global review across every output, and the verify command, which is a
//! compile without the compiler.

use super::*;
use crate::copies::Where;
use crate::processes::{Exit, Runs};
use crate::running::{Reviewed, Run};
use crate::schedule::{Done, Next, Planned, Schedule, round_of};

/// What the verifier is handed when it reviews every global criterion and test once, and the
/// batch it is run for.
// @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
pub(crate) const GLOBAL: &str = "global";

/// Whether a requirement id is a global one: a global id begins with global.
// @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
pub(crate) fn is_global(id: &str) -> bool {
    id == GLOBAL || id.starts_with("global:")
}

/// One problem per violated review: `failure at`, the file, a colon, the line, a colon, the
/// note, and the evidence in parentheses.
// @lfy def/cli/main.lfy:main#main:main:b136b63971e67fd1f1b4e0b4a407ebe829061ebda953131f2a62dc4d2993330f
pub(crate) fn problem_of(review: &Review) -> String {
    format!("failure at {}:{}: {} ({})", review.file, review.line, review.note, review.evidence)
}

/// One problem per unplaced violation: a line the verifier wrote whose status is violated but
/// which names no criterion and no test of the program, so there is nowhere to place it.
///
/// `failure at a criterion the verifier did not name`, a colon, the note of the line, and its
/// evidence in parentheses; a line that reads as no object at all is its own note.
// @lfy def/cli/main.lfy:main#main:main:c0aa127519d48d85890761c2ad8369980444d7ae53bf2834596e4e7acc7444df
pub(crate) fn problem_of_unplaced(line: &str) -> String {
    let value: Option<serde_json::Value> = serde_json::from_str(line).ok();
    let field = |name: &str| {
        value.as_ref().and_then(|value| value.get(name)).and_then(|value| value.as_str()).unwrap_or_default().to_string()
    };
    let note = {
        let note = field("note");
        if note.is_empty() { line.trim().to_string() } else { note }
    };
    // @lfy def/cli/main.lfy:main#main:main:c0aa127519d48d85890761c2ad8369980444d7ae53bf2834596e4e7acc7444df
    format!("failure at a criterion the verifier did not name: {note} ({})", field("evidence"))
}

/// Every problem one report's violations make: one per violated review, then one per unplaced
/// violation. Empty when the report violated nothing.
// @lfy def/cli/main.lfy:main#main:main:b136b63971e67fd1f1b4e0b4a407ebe829061ebda953131f2a62dc4d2993330f
// @lfy def/cli/main.lfy:main#main:main:c0aa127519d48d85890761c2ad8369980444d7ae53bf2834596e4e7acc7444df
pub(crate) fn violations_of(report: &ReviewReport) -> Vec<String> {
    // @lfy def/cli/main.lfy:main#main:main:b136b63971e67fd1f1b4e0b4a407ebe829061ebda953131f2a62dc4d2993330f
    let mut problems: Vec<String> =
        report.reviews.iter().filter(|review| review.status == ReviewStatus::Violated).map(problem_of).collect();
    // @lfy def/cli/main.lfy:main#main:main:c0aa127519d48d85890761c2ad8369980444d7ae53bf2834596e4e7acc7444df
    problems.extend(report.unplaced.iter().map(|line| problem_of_unplaced(line)));
    problems
}

/// One id the review was asked about that no review names, as the line naming it reads.
// @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
pub(crate) fn not_reviewed(id: &str) -> String {
    format!("{id} was not reviewed")
}

/// One line per id the review was asked about that no review names, so that a person reading
/// the reviewed line sees what went unanswered.
// @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
pub(crate) fn missing_lines(report: &ReviewReport) -> String {
    report.missing.iter().map(|id| not_reviewed(id)).collect::<Vec<_>>().join("\n")
}

impl Run {
    /// The reviews of one batch as one JSON array, written to
    /// `elfie-requests/<batch>.reviews.json` under the root, replacing an earlier file.
    // @lfy def/cli/main.lfy:main#main:main:a1f4dce1fbc7ba29b8c1fd6e837fee8a81c31ace4aefaabc340dbb6b9b1b0be2
    pub(crate) fn write_reviews(&mut self, batch: &Batch, reviews: &[Review]) {
        let value = serde_json::Value::Array(reviews.iter().map(Review::to_json).collect());
        let text = format!("{}\n", serde_json::to_string_pretty(&value).unwrap_or_default());
        self.write_note(batch, "reviews.json", &text);
    }

    /// The review request of a batch written for a verifier run by hand.
    // @lfy def/cli/main.lfy:main#main:main:3e861a06695127864ab425d08e899da99dbf6789c4c55135fca304c37a7e5e18
    pub(crate) fn write_review_request(&mut self, batch: &Batch) {
        let records = self.records_of(batch);
        let maps = self.maps.clone();
        let request = self.review_request_of(batch, &maps, None, &records);
        if let Some(path) = self.write_note(batch, "review.md", &request.instructions)
            && !self.json
        {
            println!("wrote {}", relative(&self.workspace().root, &path));
        }
    }

    /// What a batch's review is made with: what each of its units was last reviewed as, as the
    /// review record files in the root hold them, in batch order.
    ///
    /// With --full nothing is carried, so every criterion and test of the batch is reviewed
    /// again.
    // @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
    // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
    // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
    pub(crate) fn records_of(&self, batch: &Batch) -> Vec<ReviewRecord> {
        // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
        if self.full {
            return Vec::new();
        }
        // The records of each unit of the batch, in batch order, as the review record files
        // under the root hold them.
        // @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
        // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
        batch
            .units
            .iter()
            .flat_map(|&unit| generation::reviews_of(&self.program, &self.plan.units[unit]))
            .collect()
    }

    /// What a verifier is handed for one batch: the plan, the batch, the source maps it is
    /// pointed at, the directory its outputs were written in, and the records of its units.
    // @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
    // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
    // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
    pub(crate) fn review_request_of(
        &self,
        batch: &Batch,
        maps: &[SourceMap],
        directory: Option<&Path>,
        records: &[ReviewRecord],
    ) -> ReviewRequest {
        // @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
        generation::review(&self.program, &self.plan, batch, maps, directory, records)
    }

    /// The records a review made once more is made with: those the review before it was made
    /// with, each review that run gave replacing the record of its id, so that what was
    /// looked at and settled is carried rather than looked at a third time.
    ///
    /// With `settled`, only a review whose status is satisfied replaces a record: a run made
    /// again because it answered for too little carries what it did satisfy and nothing else.
    // @lfy def/cli/main.lfy:main#main:main:84bbcb2516b2cafb1bfbde0f772cadb0875451d4f6704a126825d45438d436dc
    // @lfy def/cli/main.lfy:main#main:main:60e9ae0b09414c3ddab6543838621c14dbb8932101affdf4f89cd4ef669d2a0b
    pub(crate) fn carried_on(
        records: &[ReviewRecord],
        request: &ReviewRequest,
        reviews: &[Review],
        settled: bool,
    ) -> Vec<ReviewRecord> {
        let mut out: Vec<ReviewRecord> = records.to_vec();
        for review in reviews {
            // @lfy def/cli/main.lfy:main#main:main:60e9ae0b09414c3ddab6543838621c14dbb8932101affdf4f89cd4ef669d2a0b
            if settled && review.status != ReviewStatus::Satisfied {
                continue;
            }
            // The record a review earns holds the hash the run's review request gave its id,
            // so a later review carries it only while the code it was judged against stands.
            // @lfy def/cli/main.lfy:main#main:main:84bbcb2516b2cafb1bfbde0f772cadb0875451d4f6704a126825d45438d436dc
            let record = ReviewRecord::of(review, &request.hashes);
            match out.iter_mut().find(|held| held.id == record.id) {
                Some(held) => *held = record,
                None => out.push(record),
            }
        }
        out
    }

    /// The review records of each unit of a batch written in the root, from what its last
    /// review found: for each local criterion and test of the unit, sorted by id, the record
    /// made from the review of its id in the batch's last verifier run, else its record among
    /// the verdicts that review request carried, else none.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    pub(crate) fn record_reviews(&mut self, batch: &Batch) {
        // Nothing reviewed the batch: there is nothing to write, so no review record file
        // changes with --no-verify, with no verifier named, or with --accept.
        // @lfy def/cli/main.lfy:main#main:main:6996e96f76cb2583f243492586dabf52cc59259bb5c5815b454493f77b21594d
        // @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
        if self.no_verify || self.accept_only || self.verifier.is_none() {
            return;
        }
        let Some(reviewed) = self.reviewed.get(&batch.identifier) else {
            return;
        };
        let mut unwritten: Vec<String> = Vec::new();
        for &index in &batch.units {
            let unit = &self.plan.units[index];
            // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
            let mut records: Vec<ReviewRecord> = Vec::new();
            for id in requirement_ids_of(&self.program, unit) {
                // The review of its id in the last verifier run, else the verdict that review
                // request carried for it, else none.
                // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
                if let Some(review) = reviewed.reviews.iter().find(|review| review.id == id) {
                    records.push(ReviewRecord::of(review, &reviewed.hashes));
                } else if let Some(carried) = reviewed.carried.iter().find(|record| record.id == id) {
                    records.push(carried.clone());
                }
            }
            // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
            if !generation::record_reviews(&self.program.workspace, unit, &records) {
                unwritten.push(generation::review_file(&self.program.workspace, unit).display().to_string());
            }
        }
        for path in unwritten {
            complain(self.style.err, Some(&path), "could not be written");
            self.worsen(ExitCode::Failure);
        }
    }

    /// The verifier run for one batch, its output shown as it arrives and logged, its report
    /// read, and its reviews written. A run that ended without every verdict is run once
    /// more, carrying what it did satisfy; when that run ends without them too its problems
    /// are printed as a failed progress line and the batch is verified with the reviews
    /// parsed from it as if its report had been complete.
    ///
    /// A review request that asks nothing runs no verifier: every verdict of the batch is
    /// carried, so there is nothing for a verifier to look at.
    ///
    /// It takes the compile locked rather than borrowed, since the lock is free while the
    /// verifier runs: that is what lets the other batches run theirs at the same time.
    // @lfy def/cli/main.lfy:main#main:main:0a2bb4066f627b6e8b99aaad5534b75619cb71fa40c84cd5ddf152a449abd6a8
    // @lfy def/cli/main.lfy:main#main:main:dc6670a1659abe98c2ae0ed1502d175a57162b9aa395c843f4fdeaa3d5ce4664
    // @lfy def/cli/main.lfy:main#main:main:49af607a5cd69c0852af2fe4631f8f52d787fd863023e06071d7569c7890cd42
    #[expect(clippy::too_many_arguments, reason = "one review of a batch takes all of this")]
    pub(crate) fn review_batch(
        run: &Mutex<Run>,
        batch: &Batch,
        command: &str,
        where_: &Where,
        progress: bool,
        maps: &[SourceMap],
        records: &[ReviewRecord],
        runs: &Runs,
        resuming: bool,
    ) -> Verification {
        // The source maps are the ones `accept` derived, held but not yet recorded, so every
        // criterion can be pointed at the region of output that claims to satisfy it, and the
        // directory is the batch's, so the regions are excerpted from the outputs the batch
        // just wrote rather than from the root's older ones.
        // @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
        let (units, request) = {
            let held = locked(run);
            let request = held.review_request_of(batch, maps, Some(where_.directory()), records);
            (stems_of(&held.plan, batch, " "), request)
        };
        // A review request that asks about nothing runs no verifier at all: every verdict of
        // the batch is carried, and those are the records its units keep.
        // @lfy def/cli/main.lfy:main#main:main:b772521c3f6da36b491eeffa1eda27e9d113f955d69a891c943d7df17113910c
        // @lfy def/cli/main.lfy:main#main:main:f79d62c52638caa4cb00a160316bc7052bc62e29caef830e683dba3fca9581ca
        // @lfy def/cli/main.lfy:main#main:main:49af607a5cd69c0852af2fe4631f8f52d787fd863023e06071d7569c7890cd42
        if request.asked.is_empty() {
            let verification = Verification { report: ReviewReport::default(), failed: false, request };
            locked(run).remember(batch, &verification);
            return verification;
        }
        // A verifying progress line is printed before the verifier runs on the batch.
        // @lfy def/cli/main.lfy:main#main:main:51dd1de1581a678bdebc04d9b69fa4f946f213ff7b9a93b18092a2e8b9587a97
        if progress {
            locked(run).reporter.report(Step::Verifying, Some(&batch.identifier), None, command);
        }
        // The verifier's run once more after the compiler's run once more is accepted goes on
        // from the session of its first run on the batch here; its own run once more for a
        // report that ended without every verdict does not.
        // @lfy def/cli/main.lfy:main#main:main:0a2bb4066f627b6e8b99aaad5534b75619cb71fa40c84cd5ddf152a449abd6a8
        // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
        let (exit, report, failed) =
            Run::ask_verifier(run, runs, command, where_, batch, &units, &request, maps, 1, resuming);
        let verification = if failed {
            // The run ended without every verdict: it is run once more, asked as it was but
            // carrying what it did satisfy.
            // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
            // @lfy def/cli/main.lfy:main#main:main:60e9ae0b09414c3ddab6543838621c14dbb8932101affdf4f89cd4ef669d2a0b
            let carried = Run::carried_on(records, &request, &report.reviews, true);
            let again = {
                let held = locked(run);
                held.review_request_of(batch, maps, Some(where_.directory()), &carried)
            };
            if again.asked.is_empty() {
                // Nothing is left to ask: what the first run reported stands as it is.
                // @lfy def/cli/main.lfy:main#main:main:3a7d4cf5d94994610376ab45fd0b8926c887075df81b180e7bd9e714d2219c33
                locked(run).note_incomplete(&batch.identifier, &report, exit.code);
                Verification { report, failed, request }
            } else {
                // The verifier's run once more after a run that ended without every verdict is
                // no resuming run.
                // @lfy def/cli/main.lfy:main#main:main:60e9ae0b09414c3ddab6543838621c14dbb8932101affdf4f89cd4ef669d2a0b
                // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
                let (exit, report, failed) =
                    Run::ask_verifier(run, runs, command, where_, batch, &units, &again, maps, 2, false);
                // A run made once more that ends without every verdict too has its problems
                // printed as a failed progress line, and what it did report is read as if it
                // had been complete.
                // @lfy def/cli/main.lfy:main#main:main:f29260af3c81a118f396bd28883ea1ce62addd257c0aced3c261874602ee375a
                // @lfy def/cli/main.lfy:main#main:main:3a7d4cf5d94994610376ab45fd0b8926c887075df81b180e7bd9e714d2219c33
                if failed {
                    locked(run).note_incomplete(&batch.identifier, &report, exit.code);
                }
                Verification { report, failed, request: again }
            }
        } else {
            Verification { report, failed, request }
        };
        let mut held = locked(run);
        // @lfy def/cli/main.lfy:main#main:main:a1f4dce1fbc7ba29b8c1fd6e837fee8a81c31ace4aefaabc340dbb6b9b1b0be2
        held.write_reviews(batch, &verification.report.reviews);
        held.remember(batch, &verification);
        drop(held);
        verification
    }

    /// What one review of a batch found, kept until its source maps are recorded, when the
    /// review records of its units are written from it.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    pub(crate) fn remember(&mut self, batch: &Batch, verification: &Verification) {
        // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
        self.reviewed.insert(
            batch.identifier.clone(),
            Reviewed {
                reviews: verification.report.reviews.clone(),
                hashes: verification.request.hashes.clone(),
                carried: verification.request.carried.clone(),
            },
        );
    }

    /// One run of the verifier on a batch: the review request as its input, the batch's
    /// directory as its directory, its output shown as it arrives and logged, and its report
    /// read with the ids that review request asks about and the reviews it submitted through
    /// the agent server.
    // @lfy def/cli/main.lfy:main#main:main:0a2bb4066f627b6e8b99aaad5534b75619cb71fa40c84cd5ddf152a449abd6a8
    // @lfy def/cli/main.lfy:main#main:main:dc6670a1659abe98c2ae0ed1502d175a57162b9aa395c843f4fdeaa3d5ce4664
    // @lfy def/cli/main.lfy:main#main:main:8e26beddf232ac15c3c4ca8bec25eccd1e26ac1300d74e2b9144e1839621effe
    #[expect(clippy::too_many_arguments, reason = "one run of the verifier takes all of this")]
    pub(crate) fn ask_verifier(
        run: &Mutex<Run>,
        runs: &Runs,
        command: &str,
        where_: &Where,
        batch: &Batch,
        units: &str,
        request: &ReviewRequest,
        maps: &[SourceMap],
        attempt: usize,
        resuming: bool,
    ) -> (Exit, ReviewReport, bool) {
        // The run is bound to the review request it is given and the source maps that request
        // was made with, so every call it makes reads the outputs it is judging.
        // @lfy def/cli/main.lfy:main#main:main:d7fdc89e00db8f2de0886cdaaecf381963abe8e1eb6392c9dee9bc26a30911b5
        let binding = {
            let held = locked(run);
            held.binding_of(
                Role::Verifier,
                Some(batch),
                where_.directory(),
                None,
                Some(request.clone()),
                maps.to_vec(),
            )
        };
        // The lock is free here, so another batch streams its own agent meanwhile.
        // @lfy def/cli/main.lfy:main#main:main:0a2bb4066f627b6e8b99aaad5534b75619cb71fa40c84cd5ddf152a449abd6a8
        let exit =
            runs.run(command, where_, &batch.identifier, units, &request.instructions, binding, resuming);
        let held = locked(run);
        let label = &batch.identifier;
        // Every line of the verifier's output and its report are appended to the log.
        // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
        held.reporter.append(&format!("--- the review of {label} (attempt {attempt}) ---\n{}", exit.stdout));
        let (report, failed) = held.report_of(&exit, &request.asked);
        (exit, report, failed)
    }

    /// The report of one run of the verifier, and whether it ended without every verdict: the
    /// whole of its standard output read with the ids its review request asked about and the
    /// reviews it submitted through the agent server.
    // @lfy def/cli/main.lfy:main#main:main:8e26beddf232ac15c3c4ca8bec25eccd1e26ac1300d74e2b9144e1839621effe
    // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
    pub(crate) fn report_of(&self, exit: &Exit, asked: &[String]) -> (ReviewReport, bool) {
        // @lfy def/cli/main.lfy:main#main:main:8e26beddf232ac15c3c4ca8bec25eccd1e26ac1300d74e2b9144e1839621effe
        let mut report = generation::review_of(&exit.stdout, &self.program, asked, &exit.record.reviews);
        if exit.code == -1 {
            report.problems.insert(0, format!("the verifier command could not be run: {}", exit.stderr.trim()));
        }
        // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
        let failed = verifier_failed(exit, &report);
        (report, failed)
    }

    /// A run that ended without every verdict: its problems are printed as a failed progress
    /// line, and what it did report is read as if it had been complete.
    // @lfy def/cli/main.lfy:main#main:main:f29260af3c81a118f396bd28883ea1ce62addd257c0aced3c261874602ee375a
    // @lfy def/cli/main.lfy:main#main:main:3a7d4cf5d94994610376ab45fd0b8926c887075df81b180e7bd9e714d2219c33
    pub(crate) fn note_incomplete(&mut self, label: &str, report: &ReviewReport, code: i32) {
        let problems = report.problems.clone();
        // @lfy def/cli/main.lfy:main#main:main:f29260af3c81a118f396bd28883ea1ce62addd257c0aced3c261874602ee375a
        self.reporter.report(Step::Failed, Some(label), None, &problems.join("\n"));
        self.reporter.problems(&problems);
        if code == -1 {
            self.failed += 1;
            self.worsen(ExitCode::Failure);
        }
    }

    /// The binding one hosted session is opened with: what the run is run as, the batch it is
    /// for, the directory it runs in, the plan and the program the command made, and what the
    /// compiler or the verifier was handed as its input.
    // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
    pub(crate) fn binding_of(
        &self,
        role: Role,
        batch: Option<&Batch>,
        directory: &Path,
        request: Option<Request>,
        review: Option<ReviewRequest>,
        source_maps: Vec<SourceMap>,
    ) -> Binding {
        // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
        Binding {
            role,
            batch: batch.cloned(),
            directory: directory.to_path_buf(),
            plan: self.plan.clone(),
            program: self.program.clone(),
            // @lfy def/cli/main.lfy:main#main:main:49b8e07b5de76a70728002b1123896a9bea926e67c9452ff6c2c821e84281a04
            request,
            review,
            source_maps,
        }
    }


    /// A batch whose units were accepted by `accept` is verified against the source maps it
    /// derived, held but not yet recorded, and the problems of its violated reviews are what
    /// it is rejected with; nothing verifies it with --no-verify or with no verifier named,
    /// and an unverifiable review is counted and never rejects.
    // @lfy def/cli/main.lfy:main#main:main:79415ab35d3d0d869ed27ece2f7e00f6466563a94f8acf3d056c77ec329cd386
    pub(crate) fn verify_batch(
        run: &Mutex<Run>,
        batch: &Batch,
        where_: &Where,
        held: &[SourceMap],
        records: &[ReviewRecord],
        runs: &Runs,
        resuming: bool,
    ) -> Option<Verified> {
        let command = {
            let state = locked(run);
            // @lfy def/cli/main.lfy:main#main:main:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
            if state.no_verify {
                return None;
            }
            state.verifier.clone()?
        };
        let verification =
            Run::review_batch(run, batch, &command, where_, true, held, records, runs, resuming);
        let report = &verification.report;
        let carried = verification.request.carried.len();
        let mut state = locked(run);
        // One reviewed line follows, whose message is the counts of satisfied, violated, and
        // unverifiable reviews of this run and then the verdicts it carried, each painted only
        // when it is worth noticing.
        // @lfy def/cli/main.lfy:main#main:main:a69cc6b70fb7a4c11a8b4d46f13c5ce8f3c5af3483523d31b91eba6e44b45af0
        // @lfy def/cli/main.lfy:main#main:main:b772521c3f6da36b491eeffa1eda27e9d113f955d69a891c943d7df17113910c
        // @lfy def/cli/main.lfy:main#main:main:f6fa7d85aff0554d36e021b666a456c9cca8d20bda732b39452c619989af78aa
        let counts = counts_with_carried(&report.reviews, report.missing.len(), carried);
        state.reporter.report_counts(Step::Reviewed, Some(&batch.identifier), &counts, None);
        // Nothing was asked: nothing was reviewed, so nothing was violated either and the
        // batch is done.
        // @lfy def/cli/main.lfy:main#main:main:b772521c3f6da36b491eeffa1eda27e9d113f955d69a891c943d7df17113910c
        if verification.request.asked.is_empty() {
            return None;
        }
        // Each id no review names is named as not reviewed, on one line under the reviewed
        // line, and counts as unverifiable above.
        // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
        if !report.missing.is_empty() {
            state.reporter.reason(&missing_lines(report));
        }
        // A run that stood had anything it wrote beside its reviews printed under its
        // reviewed line; a run that ended without every verdict had its problems printed as a
        // failed line already.
        // @lfy def/cli/main.lfy:main#main:main:7ac5c7dbdba835816dd103d3fb573c0697886fc87ae20c77a226aaa3f7b90592
        // @lfy def/cli/main.lfy:main#main:main:a353bdb9028859017c78bc8c4a41002b178de74b8ffcf75b6de2ec4d515c7bcf
        if !verification.failed {
            state.reporter.problems(&report.problems);
        }
        // An unverifiable review is counted in the reviewed line and never rejects the batch;
        // no violated review and no unplaced violation leaves the batch accepted.
        // @lfy def/cli/main.lfy:main#main:main:41f684cc85ced11c6439219ff5858457d7c2ff822185af4d70f8ce243afc79ff
        // @lfy def/cli/main.lfy:main#main:main:547a317d986ea3f6e5aa1c3f7159cf0286efabfa3aa41ee361f5f5c8c9611f52
        let problems = violations_of(report);
        if problems.is_empty() {
            return None;
        }
        // The review made once more carries what this one settled; an unplaced violation
        // names no criterion, so nothing of this review can stand and the next one carries
        // nothing at all.
        // @lfy def/cli/main.lfy:main#main:main:84bbcb2516b2cafb1bfbde0f772cadb0875451d4f6704a126825d45438d436dc
        let next = if report.unplaced.is_empty() {
            Run::carried_on(records, &verification.request, &report.reviews, false)
        } else {
            Vec::new()
        };
        Some(Verified { problems, next })
    }

    /// Every global criterion and test reviewed once for the whole program, after every batch
    /// has been handled, when a verifier is named, --no-verify is not given, and either a
    /// batch was accepted or what the last global review answered for has changed. Nothing is
    /// reviewed when the program has no global criterion and no global test, and no file is
    /// written.
    ///
    /// Gives the stems whose markers answered for a violated review, so that a compile can
    /// plan them and run them once more.
    ///
    /// It runs in the root, after every batch has been merged into it, so that what it reads
    /// is the program's whole output rather than one batch's copy.
    // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
    // @lfy def/cli/main.lfy:main#main:main:b560d79fb66587371f4a98bc89eea6db2f1123b1c6455f3310c1a9827b77b43e
    pub(crate) fn global_review(run: &Mutex<Run>, root: &Path) -> Vec<String> {
        let (agent, command, requirements, units, request, maps) = {
            let mut held = locked(run);
            // No global criterion and no global test: no global review runs and no file is
            // written.
            // @lfy def/cli/main.lfy:main#main:main:f033aae0b13c3e667e25d9d067a7b58b8f438e3b2854dcb127d56baca1010a38
            if held.program.criteria.is_empty() && held.program.tests.is_empty() {
                return Vec::new();
            }
            // @lfy def/cli/main.lfy:main#main:main:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
            if held.no_verify {
                return Vec::new();
            }
            let Some(command) = held.verifier.clone() else {
                return Vec::new();
            };
            // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
            let requirements = GlobalReview::requirements_of(&held.program);
            let last = GlobalReview::read(root);
            // Either a batch was accepted, or what the last global review answered for
            // changed.
            // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
            if !held.accepted_batch && last.as_ref().is_some_and(|last| last.requirements == requirements) {
                return Vec::new();
            }
            // A globalVerifying progress line is printed before the verifier runs.
            // @lfy def/cli/main.lfy:main#main:main:95ec4b53b789d932736cfefccb14f8a652da47d03aa0d31592172eebd66e9004
            held.reporter.report(Step::GlobalVerifying, None, None, &command);
            // Every source map, recorded or held.
            // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
            let request = generation::global_review(&held.program, &held.maps);
            // The units just generated among them.
            // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
            let units = held.global_stems();
            (held.agent.clone(), command, requirements, units, request, held.maps.clone())
        };
        // @lfy def/cli/main.lfy:main#main:main:b560d79fb66587371f4a98bc89eea6db2f1123b1c6455f3310c1a9827b77b43e
        let where_ = Where::Root(root.to_path_buf());
        // A run for the global review is no resuming run.
        // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
        let runs = Runs::new(agent, locked(run).running_of(GLOBAL));
        let report = Run::ask_globally(run, &runs, &command, &where_, &units, &request, &maps);
        let mut held = locked(run);
        held.write_global_reviews(root, &requirements, &report);
        // @lfy def/cli/main.lfy:main#main:main:2a36e0de7040861081f44ec78abe37bf9a365485f6034aa434d08e6f81d442a5
        let counts = counts_with_missing(&report.reviews, report.missing.len());
        held.reporter.report_counts(Step::GlobalReviewed, None, &counts, None);
        // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
        if !report.missing.is_empty() {
            held.reporter.reason(&missing_lines(&report));
        }
        // @lfy def/cli/main.lfy:main#main:main:37688db5376890a46f433004a7e76905a0292b3adf375e72df9a7b7ebc9ef5f5
        let violated: Vec<&Review> =
            report.reviews.iter().filter(|review| review.status == ReviewStatus::Violated).collect();
        if violated.is_empty() {
            return Vec::new();
        }
        // One problem per violated review: failure at, a space, global, a colon, a space, the
        // note, and the evidence in parentheses.
        // @lfy def/cli/main.lfy:main#main:main:6d40945fd3224481f1e733c96a5f8552118021276f2c7f21fda17341f1887ab9
        let problems: Vec<String> = violated
            .iter()
            .map(|review| format!("failure at {GLOBAL}: {} ({})", review.note, review.evidence))
            .collect();
        held.reporter.problems(&problems);
        // The units whose markers answered for it.
        // @lfy def/cli/main.lfy:main#main:main:37688db5376890a46f433004a7e76905a0292b3adf375e72df9a7b7ebc9ef5f5
        let ids: BTreeSet<String> = violated.iter().map(|review| review.id.clone()).collect();
        held.stems_answering_now(&ids)
    }

    /// The stems of every unit whose source maps hold a marker answering for one of these
    /// ids, read from the maps as they stand: a unit compiled in this run has its markers
    /// there rather than in the outputs its plan was made from.
    // @lfy def/cli/main.lfy:main#main:main:37688db5376890a46f433004a7e76905a0292b3adf375e72df9a7b7ebc9ef5f5
    pub(crate) fn stems_answering_now(&self, ids: &BTreeSet<String>) -> Vec<String> {
        // @lfy def/cli/main.lfy:main#main:main:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
        self.stems_answering_with(|id| ids.contains(id))
    }

    /// The stems, space separated, of every unit whose source maps hold a marker whose
    /// requirement is a global id: what `ELFIE_UNITS` holds for a global review.
    // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
    // @lfy def/cli/main.lfy:main#main:main:a3182832b5dbbb14cd7eb5451929bf7d8a209809b74510a1887bf3cf542b0b30
    pub(crate) fn global_stems(&self) -> String {
        // ELFIE_BATCH is global and ELFIE_UNITS the stems, separated by spaces, of every unit
        // whose source maps hold a marker answering for a global criterion or test.
        // @lfy def/cli/main.lfy:main#main:main:3ddc917bd3a57754bf0ad2a8022e78d50863262ab86f611e2e486cdb2641266f
        self.stems_answering_with(is_global).join(" ")
    }

    /// The stems of every unit whose source maps hold a marker whose requirement the test
    /// accepts.
    ///
    /// A unit's source maps are those this compile accepted for it when it has any, and
    /// [`Unit::outputs`] otherwise, so a unit generated in this compile is named by the
    /// markers it has just earned rather than by the ones the plan was made from, and a unit
    /// nothing touched by the ones its map file holds.
    // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
    pub(crate) fn stems_answering_with(&self, answers: impl Fn(&str) -> bool) -> Vec<String> {
        let mut stems: Vec<String> = Vec::new();
        for unit in &self.plan.units {
            let target = &self.program.workspace.targets[unit.target].identifier;
            let source = &self.program.workspace.files[unit.file].path;
            // A unit's source maps are those this compile accepted for it when it has any,
            // and Unit.outputs otherwise; a verify has accepted none, so there they are
            // always Unit.outputs.
            // @lfy def/cli/main.lfy:main#main:main:5701e0bba8328be1eb78fd2c1451a0191daf21f77fddf9a61cd0581350901182
            // @lfy def/cli/main.lfy:main#main:main:81c50f38555ae6dfd98db609c7d367bafaf650ea3d271e864cee62280684913d
            let accepted: Vec<&SourceMap> =
                self.maps.iter().filter(|map| &map.target == target && &map.source == source).collect();
            let maps = if accepted.is_empty() { unit.outputs.iter().collect() } else { accepted };
            // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
            let answered = maps
                .iter()
                .any(|map| map.markers.iter().any(|marker| marker.requirement.as_deref().is_some_and(&answers)));
            if answered {
                stems.push(unit.stem.clone());
            }
        }
        stems.sort();
        stems.dedup();
        stems
    }

    /// `elfie-requests/global.reviews.json` under the root written with the SHA-256 of the
    /// global ids reviewed and the report, replacing an earlier file.
    // @lfy def/cli/main.lfy:main#main:main:22a2ed809bd843b9b4106304478f1daff7eaa70e42e90d8c6d42efebc81418c7
    pub(crate) fn write_global_reviews(&mut self, root: &Path, requirements: &str, report: &ReviewReport) {
        // A GlobalReview holding that report and the SHA-256 of the global ids reviewed,
        // written where the agent server reads it from.
        // @lfy def/cli/main.lfy:main#main:main:22a2ed809bd843b9b4106304478f1daff7eaa70e42e90d8c6d42efebc81418c7
        let last = GlobalReview {
            requirements: requirements.to_string(),
            reviews: report.reviews.clone(),
            problems: report.problems.clone(),
            unplaced: report.unplaced.clone(),
            missing: report.missing.clone(),
        };
        let text = format!("{}\n", serde_json::to_string_pretty(&last.to_json()).unwrap_or_default());
        let directory = requests_directory(root);
        let path = GlobalReview::path(root);
        if let Err(error) = fs::create_dir_all(&directory).and_then(|()| fs::write(&path, text)) {
            complain(self.style.err, Some(&path.display().to_string()), &error.to_string());
            self.worsen(ExitCode::Failure);
        }
    }

    /// Every global criterion and test reviewed once for the verify command: a person asked
    /// for the opinion, so nothing but their existence gates it, and the reviews are given
    /// back to be printed.
    // @lfy def/cli/main.lfy:main#main:main:a3182832b5dbbb14cd7eb5451929bf7d8a209809b74510a1887bf3cf542b0b30
    pub(crate) fn verify_globally(run: &Mutex<Run>, root: &Path) -> Vec<Review> {
        let (agent, command, units, request, requirements, maps) = {
            let held = locked(run);
            // No global criterion and no global test: no global review runs and no file is
            // written, here as in a compile.
            // @lfy def/cli/main.lfy:main#main:main:f033aae0b13c3e667e25d9d067a7b58b8f438e3b2854dcb127d56baca1010a38
            if held.program.criteria.is_empty() && held.program.tests.is_empty() {
                return Vec::new();
            }
            let Some(command) = held.verifier.clone() else {
                return Vec::new();
            };
            // The run is one stream of the verifier, with the global review request as its
            // input and the root as its directory.
            // @lfy def/cli/main.lfy:main#main:main:9092aae70550a2dbfe8b5fed13d4592a6956f3134ba180faf2fcebe40af78f0b
            let request = generation::global_review(&held.program, &held.maps);
            // @lfy def/cli/main.lfy:main#main:main:81c50f38555ae6dfd98db609c7d367bafaf650ea3d271e864cee62280684913d
            let units = held.global_stems();
            // @lfy def/cli/main.lfy:main#main:main:a3182832b5dbbb14cd7eb5451929bf7d8a209809b74510a1887bf3cf542b0b30
            let requirements = GlobalReview::requirements_of(&held.program);
            (held.agent.clone(), command, units, request, requirements, held.maps.clone())
        };
        let where_ = Where::Root(root.to_path_buf());
        // A run for the global review is no resuming run, so it carries nothing.
        // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
        let runs = Runs::new(agent, locked(run).running_of(GLOBAL));
        let report = Run::ask_globally(run, &runs, &command, &where_, &units, &request, &maps);
        locked(run).write_global_reviews(root, &requirements, &report);
        report.reviews
    }

    /// The verifier run for the global review, at most twice, and its report: the global
    /// review request as its input and the root as its directory, with one hosted session
    /// bound to it, no batch, and the source maps that request was made with.
    // @lfy def/cli/main.lfy:main#main:main:9092aae70550a2dbfe8b5fed13d4592a6956f3134ba180faf2fcebe40af78f0b
    // @lfy def/cli/main.lfy:main#main:main:8bc948283105a23f893766f7593a57076a62f423d7ff2093b65ea6acb4853711
    // @lfy def/cli/main.lfy:main#main:main:8e26beddf232ac15c3c4ca8bec25eccd1e26ac1300d74e2b9144e1839621effe
    pub(crate) fn ask_globally(
        run: &Mutex<Run>,
        runs: &Runs,
        command: &str,
        where_: &Where,
        units: &str,
        request: &ReviewRequest,
        maps: &[SourceMap],
    ) -> ReviewReport {
        let mut last = ReviewReport::default();
        for attempt in 1..=2 {
            // The run is run as the verifier, for no batch, with the global review request it
            // is given and the source maps that request was made with.
            // @lfy def/cli/main.lfy:main#main:main:8bc948283105a23f893766f7593a57076a62f423d7ff2093b65ea6acb4853711
            let binding = {
                let held = locked(run);
                held.binding_of(Role::Verifier, None, where_.directory(), None, Some(request.clone()), maps.to_vec())
            };
            // A run for the global review is no resuming run, whatever came before it.
            // @lfy def/cli/main.lfy:main#main:main:9092aae70550a2dbfe8b5fed13d4592a6956f3134ba180faf2fcebe40af78f0b
            // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
            let exit = runs.run(command, where_, GLOBAL, units, &request.instructions, binding, false);
            let mut held = locked(run);
            // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
            held.reporter.append(&format!("--- the review of {GLOBAL} (attempt {attempt}) ---\n{}", exit.stdout));
            // @lfy def/cli/main.lfy:main#main:main:8e26beddf232ac15c3c4ca8bec25eccd1e26ac1300d74e2b9144e1839621effe
            let (report, failed) = held.report_of(&exit, &request.asked);
            last = report;
            if !failed {
                break;
            }
            if attempt == 2 {
                held.note_incomplete(GLOBAL, &last, exit.code);
            }
        }
        last
    }
}

/// What asking the verifier came to: its report, whether the run that gave it ended without
/// every verdict, so that the problems of such a run are not printed twice, and the review
/// request it was given, which says what it was asked about and what it carried.
// @lfy def/cli/main.lfy:main#main:main:a353bdb9028859017c78bc8c4a41002b178de74b8ffcf75b6de2ec4d515c7bcf
// @lfy def/cli/main.lfy:main#main:main:f6fa7d85aff0554d36e021b666a456c9cca8d20bda732b39452c619989af78aa
#[derive(Default)]
pub(crate) struct Verification {
    pub(crate) report: ReviewReport,
    pub(crate) failed: bool,
    /// The review request the run was given, or the one that asked nothing.
    // @lfy def/cli/main.lfy:main#main:main:f6fa7d85aff0554d36e021b666a456c9cca8d20bda732b39452c619989af78aa
    pub(crate) request: ReviewRequest,
}

/// What verifying one batch came to when its reviews violated what was asked: the problems
/// they make, and the records the review made once more is made with.
// @lfy def/cli/main.lfy:main#main:main:84bbcb2516b2cafb1bfbde0f772cadb0875451d4f6704a126825d45438d436dc
pub(crate) struct Verified {
    pub(crate) problems: Vec<String>,
    /// What the next review of the batch carries.
    // @lfy def/cli/main.lfy:main#main:main:84bbcb2516b2cafb1bfbde0f772cadb0875451d4f6704a126825d45438d436dc
    pub(crate) next: Vec<ReviewRecord>,
}

/// Whether a run of the verifier ended without every verdict rather than merely writing
/// something beside its reviews: it held no review at all, or it left an id it was asked
/// about unanswered and either wrote no line reading `ELFIE: REVIEWED` or ended by a signal
/// or could not be started.
///
/// A run that answered for every id it was asked about stands as it is, whatever prose
/// surrounds the answers and however it ended, and so does one that wrote the end line and
/// ended well with some of them left over: what it left is counted as unverifiable.
// @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
// @lfy def/cli/main.lfy:main#main:main:a353bdb9028859017c78bc8c4a41002b178de74b8ffcf75b6de2ec4d515c7bcf
pub(crate) fn verifier_failed(exit: &Exit, report: &ReviewReport) -> bool {
    // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
    if report.reviews.is_empty() {
        return true;
    }
    // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
    let ended = exit.code != -1 && exit.stdout.lines().any(|line| line.trim() == REVIEWED);
    !report.missing.is_empty() && !ended
}

/// The diagnostics a verify prints: the error ones alone.
// @lfy def/cli/main.lfy:main#main:main:83640c302c4ea1762f8e4981643b58c9c5094bba9180dc2995305d2406a7615c
pub(crate) fn error_diagnostics(workspace: &Workspace) -> Vec<Diagnostic> {
    query::diagnostics_of(workspace, None).into_iter().filter(|d| d.severity == Severity::Error).collect()
}

/// Every error diagnostic of the program printed; whether there was one.
// @lfy def/cli/main.lfy:main#main:main:83640c302c4ea1762f8e4981643b58c9c5094bba9180dc2995305d2406a7615c
pub(crate) fn print_errors(workspace: &Workspace, json: bool, on: bool) -> bool {
    let errors = error_diagnostics(workspace);
    for diagnostic in &errors {
        print_diagnostic(diagnostic, json, on);
    }
    !errors.is_empty()
}

/// Verify is compile without the compiler: the same plan, the same review request, the same
/// verifier, so a person can ask for an opinion on outputs already recorded. Nothing is
/// compiled and no output is written.
// @lfy def/cli/main.lfy:main#main:main:64bc2fc6236a9783f1939c6823437a0fee24fced7f789dc76b7dd418a1260f99
pub(crate) fn verify(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    // @lfy def/cli/main.lfy:main#main:main:64bc2fc6236a9783f1939c6823437a0fee24fced7f789dc76b7dd418a1260f99
    let workspace = workspace::load(root);
    let json = invocation.flag("json");
    let style = Paint::of(invocation);
    // Every error diagnostic is printed and verify ends with the code problems.
    // @lfy def/cli/main.lfy:main#main:main:83640c302c4ea1762f8e4981643b58c9c5094bba9180dc2995305d2406a7615c
    if print_errors(&workspace, json, style.out) {
        // @lfy def/cli/main.lfy:main#main:main:cd777dab37a66b80b804ad0e2c1c6eb305a85f5c6f365759da5469e81ffe4dbf
        return ExitCode::Problems;
    }
    let target = match target_named(&workspace, invocation) {
        Ok(target) => target,
        Err(code) => return code,
    };
    // Nothing under elfie-compile is recorded, moved, or removed: the maps are only read.
    // @lfy def/cli/main.lfy:main#main:main:64bc2fc6236a9783f1939c6823437a0fee24fced7f789dc76b7dd418a1260f99
    let maps = generation::source_maps_of(&workspace, target.as_deref());
    drop(workspace);
    // Plan.problems is neither printed nor counted in the exit code here, and the review goes
    // on: a person asked for an opinion on what is recorded, not for a build.
    // @lfy def/cli/main.lfy:main#main:main:304ed515dfa3121d80f29c65da75d99d67be3708e1f5c3c46961a2dbbbc46322
    let (_, known) = plan_with(root, &maps, &[], &[]);
    // Every stem given as a requested unit or, with no stem, every unit whose outputs are
    // not empty.
    // @lfy def/cli/main.lfy:main#main:main:c98c75ccd7e9b5e3cb007cb35cb2628fd4d93779a93201868a8bf2df0c6ac604
    // @lfy def/cli/main.lfy:main#main:main:d665f83cc9e0222f6d5e72fcdc1506878985d1e586251c478035d704af123680
    let mut requested: Vec<String> = Vec::new();
    // --global with no stem reviews no batch.
    // @lfy def/cli/main.lfy:main#main:main:89ce3db657a93c0c652675c6692134a009e6bf11ceecea94526a6516e7768d43
    let global_only = invocation.flag("global") && invocation.arguments.is_empty();
    // @lfy def/cli/main.lfy:main#main:main:d665f83cc9e0222f6d5e72fcdc1506878985d1e586251c478035d704af123680
    if invocation.arguments.is_empty() {
        requested.extend(known.units.iter().filter(|unit| !unit.outputs.is_empty()).map(|unit| unit.stem.clone()));
    } else {
        // @lfy def/cli/main.lfy:main#main:main:c98c75ccd7e9b5e3cb007cb35cb2628fd4d93779a93201868a8bf2df0c6ac604
        for stem in &invocation.arguments {
            // A stem naming a unit whose outputs are empty is printed as having nothing to
            // review and left out, and the code does not change for it.
            // @lfy def/cli/main.lfy:main#main:main:f6ecc4facc68022e7002a7e92ab9ba501e6cdd6242d9d87adb8b05ae09c3533f
            // @lfy def/cli/main.lfy:main#main:main:62de602001e5a8cd6c08fe0b373ce392f7dfbc93fe340e17701e10e49d405d69
            match known.units.iter().find(|unit| &unit.stem == stem) {
                Some(unit) if !unit.outputs.is_empty() => requested.push(stem.clone()),
                _ if json => println!("{}", serde_json::json!({ "stem": stem, "message": "nothing to review" })),
                _ => println!("{stem}: nothing to review"),
            }
        }
    }
    // After any batches are reviewed, every global criterion and test is reviewed once with
    // no stem given, or with --global.
    // @lfy def/cli/main.lfy:main#main:main:a3182832b5dbbb14cd7eb5451929bf7d8a209809b74510a1887bf3cf542b0b30
    let globally = invocation.flag("global") || invocation.arguments.is_empty();
    if requested.is_empty() && !globally {
        if !json {
            println!("nothing to review");
        }
        return ExitCode::Success;
    }
    let (program, plan) = plan_with(root, &maps, &requested, &[]);
    let wanted: BTreeSet<&str> = requested.iter().map(String::as_str).collect();
    // Each batch of the plan holding a requested unit, in plan order.
    // @lfy def/cli/main.lfy:main#main:main:49af607a5cd69c0852af2fe4631f8f52d787fd863023e06071d7569c7890cd42
    // @lfy def/cli/main.lfy:main#main:main:aa8d8f7253225d4bbfb5d6995399aa58057a616f7d55984c01a6687dc0ff2ee9
    let batches: Vec<usize> = (0..plan.batches.len())
        .filter(|&index| {
            let batch = &plan.batches[index];
            target.as_deref().is_none_or(|name| batch_target(&program.workspace, &plan, batch) == name)
                && batch.units.iter().any(|&unit| wanted.contains(plan.units[unit].stem.as_str()))
        })
        .collect();
    let total: usize = batches.iter().map(|&index| plan.batches[index].units.len()).sum();
    let reporter = Reporter::new(json, style, total, log_paths(root));
    let run = Mutex::new(Run::new(program, plan, maps, reporter, invocation));
    let verifier = verifier_command(root);
    // At most --jobs batches are reviewed at once.
    // @lfy def/cli/main.lfy:main#main:main:d70597c60c888f5f421bf553ba38472c5a50057472a0e699ac64627f24b6c8c7
    let jobs = jobs_of(invocation);
    // The source maps already recorded are what the verifier is pointed at here: verify
    // compiles nothing, so there are none to hold.
    // @lfy def/cli/main.lfy:main#main:main:64bc2fc6236a9783f1939c6823437a0fee24fced7f789dc76b7dd418a1260f99
    let recorded = locked(&run).maps.clone();
    // @lfy def/cli/main.lfy:main#main:main:89ce3db657a93c0c652675c6692134a009e6bf11ceecea94526a6516e7768d43
    let wanted = if global_only { Vec::new() } else { batches };
    // Each batch of the plan holding a requested unit, up to --jobs at once and started in
    // plan order.
    // @lfy def/cli/main.lfy:main#main:main:49af607a5cd69c0852af2fe4631f8f52d787fd863023e06071d7569c7890cd42
    let reviewing = round_of(&locked(&run).plan, &wanted);
    verify_batches(&run, &reviewing, root, verifier.as_deref(), &recorded, jobs);
    // @lfy def/cli/main.lfy:main#main:main:a3182832b5dbbb14cd7eb5451929bf7d8a209809b74510a1887bf3cf542b0b30
    if globally && verifier.is_some() {
        // Nothing of the compile's own gating applies here: a person asked for the opinion.
        // @lfy def/cli/main.lfy:main#main:main:a3182832b5dbbb14cd7eb5451929bf7d8a209809b74510a1887bf3cf542b0b30
        let reviews = Run::verify_globally(&run, root);
        // A global review carries nothing: every global criterion and test is reviewed once.
        // @lfy def/cli/main.lfy:main#main:main:a3182832b5dbbb14cd7eb5451929bf7d8a209809b74510a1887bf3cf542b0b30
        print_reviews(&reviews, &[], 0, json, style.out);
        // @lfy def/cli/main.lfy:main#main:main:e66e42fc4344ab7898da31692b13cfa9d1621e6c695a0226fd0e42bb5522bdd4
        if counts_of(&reviews).1 > 0 {
            locked(&run).worsen(ExitCode::Problems);
        }
    }
    // Every session of this verify is ended and the server stopped, after the last run has
    // returned and before the command returns, whatever the outcome.
    // @lfy def/cli/main.lfy:main#main:main:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
    let server = locked(&run).agent.server.clone();
    server.stop();
    // No review is violated and the verifier was started, or no review request asked
    // anything: the code is success; it could not be started: the code is failure.
    // @lfy def/cli/main.lfy:main#main:main:c3a2ad3cc8b8443eed86ed501261573e2799abb00383db190b158541be935033
    // @lfy def/cli/main.lfy:main#main:main:f4d733d1c98fcb52efb7af2bcf5a54e5df115f3454b8c0ce42aa336de6dd2857
    locked(&run).code
}

/// Every batch of a verify reviewed, up to --jobs at once and started in plan order, each in
/// the root: nothing is compiled, so every batch reads the outputs that are already there.
///
/// With no verifier named, the review request of each batch is written for a verifier run by
/// hand and the code is success.
// @lfy def/cli/main.lfy:main#main:main:49af607a5cd69c0852af2fe4631f8f52d787fd863023e06071d7569c7890cd42
pub(crate) fn verify_batches(
    run: &Mutex<Run>,
    planned: &[Planned],
    root: &Path,
    verifier: Option<&str>,
    recorded: &[SourceMap],
    jobs: usize,
) {
    let schedule = Mutex::new(Schedule::of(planned));
    let finished = Condvar::new();
    std::thread::scope(|scope| {
        let mut running = Vec::new();
        loop {
            let mut schedules = schedule.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // @lfy def/cli/main.lfy:main#main:main:49af607a5cd69c0852af2fe4631f8f52d787fd863023e06071d7569c7890cd42
            match schedules.next(planned, jobs, false, false) {
                Next::Start(at) => {
                    drop(schedules);
                    let batch = planned[at].batch.clone();
                    let schedule = &schedule;
                    let finished = &finished;
                    running.push(scope.spawn(move || {
                        match verifier {
                            // @lfy def/cli/main.lfy:main#main:main:49af607a5cd69c0852af2fe4631f8f52d787fd863023e06071d7569c7890cd42
                            Some(command) => {
                                // The run is one stream of the command, with the review
                                // request as its input and the root as its directory; a run
                                // that ended without every verdict is run once more.
                                // @lfy def/cli/main.lfy:main#main:main:dc6670a1659abe98c2ae0ed1502d175a57162b9aa395c843f4fdeaa3d5ce4664
                                // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
                                let where_ = Where::Root(root.to_path_buf());
                                // What the batch's review carries: what each of its units was
                                // last reviewed as, and nothing at all with --full.
                                // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
                                // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
                                let records = locked(run).records_of(&batch);
                                // A verify's run of the verifier is a first run, so it goes
                                // on from nothing.
                                // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
                                let runs = {
                                    let mut state = locked(run);
                                    let agent = state.agent.clone();
                                    Runs::new(agent, state.running_of(&batch.identifier))
                                };
                                let verification = Run::review_batch(
                                    run, &batch, command, &where_, false, recorded, &records, &runs, false,
                                );
                                let carried = verification.request.carried.len();
                                let report = verification.report;
                                let reviews = report.reviews;
                                let mut state = locked(run);
                                // Every review printed, then the counts, then the id of every
                                // criterion and test the review left unanswered. A review
                                // request that asked nothing ran no verifier, so it prints no
                                // review and counts none.
                                // @lfy def/cli/main.lfy:main#main:main:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
                                // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
                                // @lfy def/cli/main.lfy:main#main:main:f79d62c52638caa4cb00a160316bc7052bc62e29caef830e683dba3fca9581ca
                                // @lfy def/cli/main.lfy:main#main:main:2edbac225cec2318ff261227fbb09c7256786df4743f9748be8742962271a3ac
                                print_reviews(&reviews, &report.missing, carried, state.json, state.style.out);
                                // @lfy def/cli/main.lfy:main#main:main:e66e42fc4344ab7898da31692b13cfa9d1621e6c695a0226fd0e42bb5522bdd4
                                if counts_of(&reviews).1 > 0 {
                                    state.worsen(ExitCode::Problems);
                                }
                            }
                            // @lfy def/cli/main.lfy:main#main:main:3e861a06695127864ab425d08e899da99dbf6789c4c55135fca304c37a7e5e18
                            // @lfy def/cli/main.lfy:main#main:main:b6938589b155e207c8b6d2d220a8d8726be7f451d3867f2a1d037c89951d9923
                            None => locked(run).write_review_request(&batch),
                        }
                        schedule
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .finish(at, &Done::Merged);
                        finished.notify_all();
                    }));
                }
                Next::Wait => {
                    drop(finished.wait(schedules));
                }
                Next::Over => break,
            }
        }
        for handle in running {
            let _ = handle.join();
        }
    });
}

/// Every review printed as its status, a space, the file, a colon, the line, a space, the
/// entity, a colon, a space, and the note, then one line giving the counts of satisfied,
/// violated, and unverifiable. Without --json the status is painted in its own tone and padded
/// to the longest status of [`ReviewStatus`], the file and line are subject, and the entity and
/// the note are plain; with --json each review is one JSON object on one line.
///
/// Every id the review was asked about that no review names is counted among the unverifiable
/// and then named, one line under the counts line, as not reviewed; every verdict the review
/// request carried is counted after them, and only there.
// @lfy def/cli/main.lfy:main#main:main:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
// @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
// @lfy def/cli/main.lfy:main#main:main:2edbac225cec2318ff261227fbb09c7256786df4743f9748be8742962271a3ac
pub(crate) fn print_reviews(reviews: &[Review], unanswered: &[String], carried: usize, json: bool, on: bool) {
    let missing = unanswered.len();
    for review in reviews {
        // @lfy def/cli/main.lfy:main#main:main:639501265f62b510499fe150b611345c387104c04de980656103153401de8d17
        if json {
            println!("{}", review.to_json());
        } else {
            // @lfy def/cli/main.lfy:main#main:main:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
            println!("{}", painted_review(review, on));
        }
    }
    // After each batch one line gives the counts of satisfied, violated, and unverifiable,
    // every id no review names counted among the unverifiable.
    // @lfy def/cli/main.lfy:main#main:main:a401e7319fbb4bdf617fc08adbbe3fea18e30d1c519f1fcbf538029c7dc283a2
    // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
    let counts = counts_of(reviews);
    if json {
        println!(
            "{}",
            serde_json::json!({
                "satisfied": counts.0,
                "violated": counts.1,
                "unverifiable": counts.2 + missing,
                // @lfy def/cli/main.lfy:main#main:main:cc320fe0b40fde7c47dca674e64001b0a974e93cf2ffa2186a7c30bbf7aad8f7
                "carried": carried,
            })
        );
    } else {
        // The counts line begins with the satisfied, the violated, and the unverifiable, and
        // ends with the verdicts the review request carried when it carried any.
        // @lfy def/cli/main.lfy:main#main:main:5e715c81a6d8d3edeadda85d5a0a8cc96d3f5aa33c9009f7b213ae941ccb4eff
        // @lfy def/cli/main.lfy:main#main:main:cc320fe0b40fde7c47dca674e64001b0a974e93cf2ffa2186a7c30bbf7aad8f7
        println!("{}", counts_message(&counts_with_carried(reviews, missing, carried), on));
        // Each id no review names is named as not reviewed, on one line under the counts
        // line, and counts as unverifiable above.
        // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
        for id in unanswered {
            println!("{INDENT}{}", paint(&not_reviewed(id), Tone::Warning, on));
        }
    }
}

/// One review as the verify command prints it, painted.
// @lfy def/cli/main.lfy:main#main:main:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
pub(crate) fn painted_review(review: &Review, on: bool) -> String {
    // The status is padded to the longest status of ReviewStatus.
    // @lfy def/cli/main.lfy:main#main:main:0011d65e2e66e2fa6bffcf28bb68b59c7d3425bddda6fd61a196e714724e5195
    let column = [ReviewStatus::Satisfied, ReviewStatus::Violated, ReviewStatus::Unverifiable]
        .iter()
        .map(|status| width(&status.to_string()))
        .max()
        .unwrap_or(0);
    // The status is painted in its own tone, the file and line are subject, and the entity
    // and the note are plain.
    // @lfy def/cli/main.lfy:main#main:main:0011d65e2e66e2fa6bffcf28bb68b59c7d3425bddda6fd61a196e714724e5195
    let status = padded(&paint(&review.status.to_string(), status_tone(review.status), on), column);
    format!(
        "{status} {} {}: {}",
        paint(&format!("{}:{}", review.file, review.line), Tone::Subject, on),
        review.entity,
        review.note
    )
}
