// @lfy def/cli/compile.lfy:compile
//! Reviewing what was compiled: the review request of one unit, the verifier runs that answer
//! it, the global review across every output, and the verify command, which is a compile
//! without the compiler.

use super::*;
use crate::processes::{Exit, Runs};
use crate::running::{Reviewed, Run};
use crate::schedule::Verified;

/// What the verifier is handed when it reviews every global criterion and test once, and the
/// name its run goes by.
// @lfy def/cli/compile.lfy:compile#compile:compile:73c389b755f64b116aefe36d13a3dbbe224b401808c21b5c6e58de07a4b1f1d6
pub(crate) const GLOBAL: &str = "global";

/// One problem per violated review: `failure at`, the file, a colon, the line, a colon, the
/// note, and the evidence in parentheses.
// @lfy def/cli/compile.lfy:compile#compile:compile:8533b0690caaa0d5a3703e6633b44fd6034ce112c1a2c32f151e384e973b8bdf
pub(crate) fn problem_of(review: &Review) -> String {
    format!("failure at {}:{}: {} ({})", review.file, review.line, review.note, review.evidence)
}

/// One problem per unplaced violation: a line the verifier wrote whose status is violated but
/// which names no criterion and no test of the program, so there is nowhere to place it.
///
/// `failure at a criterion the verifier did not name`, a colon, the note of the line, and its
/// evidence in parentheses; a line that reads as no object at all is its own note.
// @lfy def/cli/compile.lfy:compile#compile:compile:6c97f065ec22808dd6a71afa10fbb1b9f2c63c5fe6ab869aeeb129eeb68a59d1
pub(crate) fn problem_of_unplaced(line: &str) -> String {
    let value: Option<serde_json::Value> = serde_json::from_str(line).ok();
    let field = |name: &str| {
        value.as_ref().and_then(|value| value.get(name)).and_then(|value| value.as_str()).unwrap_or_default().to_string()
    };
    let note = {
        let note = field("note");
        if note.is_empty() { line.trim().to_string() } else { note }
    };
    // @lfy def/cli/compile.lfy:compile#compile:compile:6c97f065ec22808dd6a71afa10fbb1b9f2c63c5fe6ab869aeeb129eeb68a59d1
    format!("failure at a criterion the verifier did not name: {note} ({})", field("evidence"))
}

/// Every problem one report's violations make: one per violated review, then one per unplaced
/// violation. Empty when the report violated nothing.
// @lfy def/cli/compile.lfy:compile#compile:compile:8533b0690caaa0d5a3703e6633b44fd6034ce112c1a2c32f151e384e973b8bdf
// @lfy def/cli/compile.lfy:compile#compile:compile:6c97f065ec22808dd6a71afa10fbb1b9f2c63c5fe6ab869aeeb129eeb68a59d1
pub(crate) fn violations_of(report: &ReviewReport) -> Vec<String> {
    let mut problems: Vec<String> =
        report.reviews.iter().filter(|review| review.status == ReviewStatus::Violated).map(problem_of).collect();
    problems.extend(report.unplaced.iter().map(|line| problem_of_unplaced(line)));
    problems
}

/// The one line naming every id the review was asked about that no review names, each as not
/// reviewed; nothing when there is none.
// @lfy def/cli/compile.lfy:compile#compile:compile:97ca8a3e66805eba7104729c205da94cf64474677a3c53e056be523758150d9d
pub(crate) fn not_reviewed(ids: &[String]) -> String {
    if ids.is_empty() {
        return String::new();
    }
    format!("not reviewed: {}", ids.join(", "))
}

/// The one line under the reviewed line naming what went unanswered, so that a person reading
/// it sees every id no review names.
// @lfy def/cli/compile.lfy:compile#compile:compile:97ca8a3e66805eba7104729c205da94cf64474677a3c53e056be523758150d9d
pub(crate) fn missing_lines(report: &ReviewReport) -> String {
    not_reviewed(&report.missing)
}

impl Run {
    /// The reviews of one unit as one JSON array, written to
    /// `elfie-requests/<name>.reviews.json` under the root, with `<name>` the unit's request
    /// name, replacing an earlier file.
    // @lfy def/cli/compile.lfy:compile#compile:compile:15c52319c401850d74a412fdfeffa9885cf323b190149210e6e0a83cf4d62156
    pub(crate) fn write_reviews(&mut self, unit: usize, reviews: &[Review]) {
        let value = serde_json::Value::Array(reviews.iter().map(Review::to_json).collect());
        let text = format!("{}\n", serde_json::to_string_pretty(&value).unwrap_or_default());
        let stem = self.stem(unit);
        self.write_note(&stem, "reviews.json", &text);
    }

    /// The review request of a unit written to `elfie-requests/<name>.review.md` for a
    /// verifier run by hand.
    // @lfy def/cli/compile.lfy:verify#verify:verify:4433e487958c0421f6e9121d7dec547034bf56c187a29d2478a9f228caba4eb4
    pub(crate) fn write_review_request(&mut self, unit: usize) {
        let root = self.root();
        let records = self.verify_records(unit, &root);
        let maps = self.maps.clone();
        let request = self.review_request_of(unit, &maps, &root, &records);
        let stem = self.stem(unit);
        if let Some(path) = self.write_note(&stem, "review.md", &request.instructions)
            && !self.json
        {
            println!("wrote {}", relative(&self.workspace().root, &path));
        }
    }

    /// What a unit's review is made with: what it was last reviewed as, as `reviewsOf` reads
    /// its review record file in the unit's directory.
    ///
    /// With --full nothing is carried, so every criterion and test of the unit is reviewed
    /// again.
    // @lfy def/cli/compile.lfy:compile#compile:compile:291055dad770c8b6cca4ad39374f2decfc9ad7211f3a98cda19486467b3c72be
    // @lfy def/cli/compile.lfy:verify#verify:verify:9fc0da16b4fb4241e8c2e859851d9e3fb85f014aa0a2f7a997decf40c5da9d86
    pub(crate) fn records_of(&mut self, unit: usize, directory: &Path) -> Vec<ReviewRecord> {
        // @lfy def/cli/compile.lfy:verify#verify:verify:9fc0da16b4fb4241e8c2e859851d9e3fb85f014aa0a2f7a997decf40c5da9d86
        if self.full {
            return Vec::new();
        }
        let planned = self.plan.units[unit].clone();
        // @lfy def/cli/compile.lfy:compile#compile:compile:291055dad770c8b6cca4ad39374f2decfc9ad7211f3a98cda19486467b3c72be
        generation::reviews_of(self.program_at(directory), &planned)
    }

    /// What verify reviews a unit with: for the unit of one of the review records verify's
    /// relocation gives, the records of that unit's, which nothing is written of; otherwise what
    /// `reviewsOf` gives for it; and nothing at all with --full.
    // @lfy def/cli/compile.lfy:verify#verify:verify:62ee8e26855d189d744062b262cfed91d039f0cd61f31e843738d7a60ec73d44
    // @lfy def/cli/compile.lfy:verify#verify:verify:9fc0da16b4fb4241e8c2e859851d9e3fb85f014aa0a2f7a997decf40c5da9d86
    pub(crate) fn verify_records(&mut self, unit: usize, directory: &Path) -> Vec<ReviewRecord> {
        if self.full {
            return Vec::new();
        }
        let target = self.target_of_unit(unit).identifier.clone();
        let stem = self.stem(unit);
        // @lfy def/cli/compile.lfy:verify#verify:verify:62ee8e26855d189d744062b262cfed91d039f0cd61f31e843738d7a60ec73d44
        match self.relocation.reviews.iter().find(|reviews| reviews.target == target && reviews.stem == stem) {
            Some(reviews) => reviews.records.clone(),
            None => self.records_of(unit, directory),
        }
    }

    /// What a verifier is handed for one unit: the plan, the unit, the source maps it is
    /// pointed at, the directory its outputs were written in, and its records.
    // @lfy def/cli/compile.lfy:compile#compile:compile:291055dad770c8b6cca4ad39374f2decfc9ad7211f3a98cda19486467b3c72be
    // @lfy def/cli/compile.lfy:verify#verify:verify:62ee8e26855d189d744062b262cfed91d039f0cd61f31e843738d7a60ec73d44
    pub(crate) fn review_request_of(
        &self,
        unit: usize,
        maps: &[SourceMap],
        directory: &Path,
        records: &[ReviewRecord],
    ) -> ReviewRequest {
        // @lfy def/cli/compile.lfy:compile#compile:compile:291055dad770c8b6cca4ad39374f2decfc9ad7211f3a98cda19486467b3c72be
        generation::review(&self.program, &self.plan, unit, maps, Some(directory), records)
    }

    /// The records a review made once more is made with: those the review before it was made
    /// with, each review that run gave replacing the record of its id, so that what was
    /// looked at and settled is carried rather than looked at a third time.
    ///
    /// With `settled`, only a review whose status is satisfied replaces a record: a run made
    /// again because it answered for too little carries what it did satisfy and nothing else.
    // @lfy def/cli/compile.lfy:compile#compile:compile:c3711c547c11411e55264eed71019f21008660209ec5348f9c940bbad6ddeb23
    // @lfy def/cli/compile.lfy:compile#compile:compile:14202a700387b5a57301e7eb3beb6939ee17d21521f15fc7ad17f72e1bd21012
    pub(crate) fn carried_on(
        records: &[ReviewRecord],
        request: &ReviewRequest,
        reviews: &[Review],
        settled: bool,
    ) -> Vec<ReviewRecord> {
        let mut out: Vec<ReviewRecord> = records.to_vec();
        for review in reviews {
            // @lfy def/cli/compile.lfy:compile#compile:compile:14202a700387b5a57301e7eb3beb6939ee17d21521f15fc7ad17f72e1bd21012
            if settled && review.status != ReviewStatus::Satisfied {
                continue;
            }
            // The record a review earns holds the hash the run's review request gave its id,
            // so a later review carries it only while the code it was judged against stands.
            // @lfy def/cli/compile.lfy:compile#compile:compile:c3711c547c11411e55264eed71019f21008660209ec5348f9c940bbad6ddeb23
            let record = ReviewRecord::of(review, &request.hashes);
            match out.iter_mut().find(|held| held.id == record.id) {
                Some(held) => *held = record,
                None => out.push(record),
            }
        }
        out
    }

    /// The review records of a unit written in its directory by `recordReviews`, from what its
    /// last review found: for each local criterion and test of the unit, sorted by id, the
    /// record made from the review of its id in the unit's last verifier run, else its record
    /// among the verdicts that review request carried, else none.
    // @lfy def/cli/compile.lfy:compile#compile:compile:ce733e8816d1638bf89269929d621f9cc341aaee184834fbd66203ed30bfb6b1
    // @lfy def/cli/compile.lfy:compile#compile:compile:6996e96f76cb2583f243492586dabf52cc59259bb5c5815b454493f77b21594d
    // @lfy def/cli/compile.lfy:compile#compile:compile:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
    pub(crate) fn record_reviews(&mut self, unit: usize, directory: &Path) {
        // Nothing reviewed the unit: there is nothing to write, so no review record file
        // changes with --no-verify, with no verifier named, or with --accept.
        // @lfy def/cli/compile.lfy:compile#compile:compile:6996e96f76cb2583f243492586dabf52cc59259bb5c5815b454493f77b21594d
        // @lfy def/cli/compile.lfy:compile#compile:compile:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
        if !self.verifies() || self.accept_only {
            return;
        }
        let Some(reviewed) = self.reviewed.get(&unit).cloned() else {
            return;
        };
        let planned = self.plan.units[unit].clone();
        let mut records: Vec<ReviewRecord> = Vec::new();
        for id in requirement_ids_of(&self.program, &planned) {
            // @lfy def/cli/compile.lfy:compile#compile:compile:ce733e8816d1638bf89269929d621f9cc341aaee184834fbd66203ed30bfb6b1
            if let Some(review) = reviewed.reviews.iter().find(|review| review.id == id) {
                records.push(ReviewRecord::of(review, &reviewed.hashes));
            } else if let Some(carried) = reviewed.carried.iter().find(|record| record.id == id) {
                records.push(carried.clone());
            }
        }
        let program = self.program_at(directory);
        // @lfy def/cli/compile.lfy:compile#compile:compile:e80072c3cfb6ab1e3a597e86b089de8c771b5070f30422ff29897adad052dbdc
        let written = generation::record_reviews(&program.workspace, &planned, &records);
        let path = generation::review_file(&program.workspace, &planned).display().to_string();
        if !written {
            complain(self.style.err, Some(&path), "could not be written");
            self.worsen(ExitCode::Failure);
        }
    }

    /// The verifier run for one unit, its output shown as it arrives and logged, its report
    /// read, and its reviews written. A run that ended without every verdict is run once
    /// more, carrying what it did satisfy; when that run ends without them too its problems
    /// are printed as a failed progress line and the unit is verified with the reviews parsed
    /// from it as if its report had been complete.
    ///
    /// A review request that asks nothing runs no verifier: every verdict of the unit is
    /// carried, so there is nothing for a verifier to look at.
    // @lfy def/cli/compile.lfy:compile#compile:compile:70f1da8e3f20d07139294c2c514b6b9800db1030922bf13ff43f53df89919a4e
    // @lfy def/cli/compile.lfy:verify#verify:verify:4185b35d14b24c85bfd17b51dfbe20df36cada069c9fb42bf4e35263334a5d97
    // @lfy def/cli/compile.lfy:compile#compile:compile:dbcb80625881c43a2f36b95af5c8519113c0f1736c3c3171e2e3f55e426bd6d7
    #[expect(clippy::too_many_arguments, reason = "one review of a unit takes all of this")]
    pub(crate) fn review_unit(
        &mut self,
        unit: usize,
        command: &str,
        directory: &Path,
        progress: bool,
        maps: &[SourceMap],
        records: &[ReviewRecord],
        runs: &Runs,
        resuming: bool,
    ) -> Verification {
        // The source maps are the ones `accept` derived, not yet recorded, so every criterion
        // can be pointed at the region of output that claims to satisfy it, and the directory
        // is the unit's, so the regions are excerpted from the outputs it just wrote.
        // @lfy def/cli/compile.lfy:compile#compile:compile:291055dad770c8b6cca4ad39374f2decfc9ad7211f3a98cda19486467b3c72be
        let request = self.review_request_of(unit, maps, directory, records);
        // A review request that asks about nothing runs no verifier at all.
        // @lfy def/cli/compile.lfy:compile#compile:compile:14cbfe6603ade5c97b980e258fee2c6beaff3dce14dd1ea1d7e29c34507da39f
        // @lfy def/cli/compile.lfy:verify#verify:verify:1210eb92f28da423c86881acae7b44dcc7eb7346006e861a85cef47a910c5f8f
        if request.asked.is_empty() {
            let verification = Verification { report: ReviewReport::default(), failed: false, request };
            self.remember(unit, &verification);
            return verification;
        }
        let stem = self.stem(unit);
        // A verifying progress line is printed before the verifier runs on the unit.
        // @lfy def/cli/compile.lfy:compile#compile:compile:3504f3ea7a1558d834cec2aa56168a829809b825cf4b77bfc335faf2484e2ca8
        if progress {
            self.reporter.report(Step::Verifying, Some(&stem), command);
        }
        // The verifier's run once more after the compiler's run once more is accepted goes on
        // from the session of its first run on the unit; its own run once more for a report
        // that ended without every verdict does not.
        // @lfy def/cli/compile.lfy:compile#compile:compile:70f1da8e3f20d07139294c2c514b6b9800db1030922bf13ff43f53df89919a4e
        // @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
        let (exit, report, failed) = self.ask_verifier(runs, command, directory, unit, &request, maps, resuming);
        let verification = if failed && !self.halted {
            // The run ended without every verdict: it is run once more, asked as it was but
            // carrying what it did satisfy.
            // @lfy def/cli/compile.lfy:compile#compile:compile:dbcb80625881c43a2f36b95af5c8519113c0f1736c3c3171e2e3f55e426bd6d7
            // @lfy def/cli/compile.lfy:compile#compile:compile:14202a700387b5a57301e7eb3beb6939ee17d21521f15fc7ad17f72e1bd21012
            let carried = Run::carried_on(records, &request, &report.reviews, true);
            let again = self.review_request_of(unit, maps, directory, &carried);
            if again.asked.is_empty() {
                // Nothing is left to ask: what the first run reported stands as it is.
                // @lfy def/cli/compile.lfy:compile#compile:compile:4db6841d8a4555959bbe4c0ce1755f3815ad577d52bc801c77c3a370bc79872e
                self.note_incomplete(&stem, &report, exit.code);
                Verification { report, failed, request }
            } else {
                // The verifier's run once more after a run that ended without every verdict is
                // no resuming run.
                // @lfy def/cli/compile.lfy:compile#compile:compile:9e092cdf07f0cd7a1468317b2e75bb10ee5513015372fa964ca1fdff2d436dba
                let (exit, report, failed) = self.ask_verifier(runs, command, directory, unit, &again, maps, false);
                // A run made once more that ends without every verdict too has its problems
                // printed as a failed progress line, and what it did report is read as if it
                // had been complete.
                // @lfy def/cli/compile.lfy:compile#compile:compile:85f1d3ef649f3839f3e1f7f75cc2c0c880e5b1c1b9978b38bddb4b309b264f1b
                // @lfy def/cli/compile.lfy:compile#compile:compile:4db6841d8a4555959bbe4c0ce1755f3815ad577d52bc801c77c3a370bc79872e
                if failed {
                    self.note_incomplete(&stem, &report, exit.code);
                }
                Verification { report, failed, request: again }
            }
        } else {
            Verification { report, failed, request }
        };
        // @lfy def/cli/compile.lfy:compile#compile:compile:15c52319c401850d74a412fdfeffa9885cf323b190149210e6e0a83cf4d62156
        self.write_reviews(unit, &verification.report.reviews);
        self.remember(unit, &verification);
        verification
    }

    /// What one review of a unit found, kept until its source maps are recorded, when its
    /// review records are written from it.
    // @lfy def/cli/compile.lfy:compile#compile:compile:ce733e8816d1638bf89269929d621f9cc341aaee184834fbd66203ed30bfb6b1
    pub(crate) fn remember(&mut self, unit: usize, verification: &Verification) {
        self.reviewed.insert(
            unit,
            Reviewed {
                reviews: verification.report.reviews.clone(),
                hashes: verification.request.hashes.clone(),
                carried: verification.request.carried.clone(),
            },
        );
    }

    /// One run of the verifier on a unit: the review request as its input, the unit's
    /// directory as its directory, its output shown as it arrives and logged, and its report
    /// read with the ids that review request asks about and the reviews it submitted through
    /// the agent server. The run has the role verifier, the unit, the review request given as
    /// its input, and the source maps that review request was made with.
    // @lfy def/cli/compile.lfy:compile#compile:compile:70f1da8e3f20d07139294c2c514b6b9800db1030922bf13ff43f53df89919a4e
    // @lfy def/cli/compile.lfy:compile#compile:compile:a4c3ad2a056f59fac7b6a9db55c7289d1c10eb4f174437ea829a0b82e2d58f92
    // @lfy def/cli/compile.lfy:compile#compile:compile:42133b06924184d8399eb7aa96f210caf2e4dfcca35cbe9697330cdfb149bee5
    #[expect(clippy::too_many_arguments, reason = "one run of the verifier takes all of this")]
    pub(crate) fn ask_verifier(
        &mut self,
        runs: &Runs,
        command: &str,
        directory: &Path,
        unit: usize,
        request: &ReviewRequest,
        maps: &[SourceMap],
        resuming: bool,
    ) -> (Exit, ReviewReport, bool) {
        // @lfy def/cli/compile.lfy:compile#compile:compile:a4c3ad2a056f59fac7b6a9db55c7289d1c10eb4f174437ea829a0b82e2d58f92
        let binding = self.binding_of(Role::Verifier, Some(unit), directory, None, None, Some(request.clone()), maps.to_vec());
        let stem = self.stem(unit);
        let exit = runs.run(command, directory, &stem, &request.instructions, binding, resuming);
        // Every line of the verifier's output, its report, reached the log as it was shown.
        // @lfy def/cli/main.lfy:main#main:main:3082965661f2495d1e5e9d8a2177fc2ca759320db1b83c7e7bc0605b9be65f3f
        let (report, failed) = self.report_of(&exit, &request.asked);
        (exit, report, failed)
    }

    /// The report of one run of the verifier, and whether it ended without every verdict: the
    /// whole of its standard output read by `reviewOf` with the ids its review request asked
    /// about and the reviews it submitted through the agent server.
    // @lfy def/cli/compile.lfy:compile#compile:compile:42133b06924184d8399eb7aa96f210caf2e4dfcca35cbe9697330cdfb149bee5
    // @lfy def/cli/compile.lfy:compile#compile:compile:dbcb80625881c43a2f36b95af5c8519113c0f1736c3c3171e2e3f55e426bd6d7
    pub(crate) fn report_of(&self, exit: &Exit, asked: &[String]) -> (ReviewReport, bool) {
        // @lfy def/cli/compile.lfy:compile#compile:compile:42133b06924184d8399eb7aa96f210caf2e4dfcca35cbe9697330cdfb149bee5
        let mut report = generation::review_of(&exit.stdout, &self.program, asked, &exit.record.reviews);
        if exit.code == -1 {
            report.problems.insert(0, format!("the verifier command could not be run: {}", exit.stderr.trim()));
        }
        let failed = verifier_failed(exit, &report);
        (report, failed)
    }

    /// A run that ended without every verdict: its problems are printed as a failed progress
    /// line, and what it did report is read as if it had been complete.
    // @lfy def/cli/compile.lfy:compile#compile:compile:85f1d3ef649f3839f3e1f7f75cc2c0c880e5b1c1b9978b38bddb4b309b264f1b
    pub(crate) fn note_incomplete(&mut self, label: &str, report: &ReviewReport, code: i32) {
        let problems = report.problems.clone();
        // @lfy def/cli/compile.lfy:compile#compile:compile:85f1d3ef649f3839f3e1f7f75cc2c0c880e5b1c1b9978b38bddb4b309b264f1b
        let unit = (label != GLOBAL).then_some(label);
        // @lfy def/cli/compile.lfy:compile#compile:compile:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
        self.reporter.report_problems(Step::Failed, unit, &problems.join("\n"), &problems);
        if code == -1 {
            self.worsen(ExitCode::Failure);
        }
    }

    /// A unit whose outputs were accepted by `accept` is verified against the source maps it
    /// derived, not yet recorded, and the problems of its violated reviews are what it is
    /// rejected with; an unverifiable review is counted and never rejects.
    ///
    /// One reviewed progress line follows the review, whose message is the counts of
    /// satisfied, violated, and unverifiable reviews of this run and then the verdicts it
    /// carried, each painted only when it is worth noticing.
    // @lfy def/cli/compile.lfy:compile#compile:compile:e043cf95bc9cd594a76db11930ff69d434f4eafa3b00576563e48b0caf192912
    // @lfy def/cli/compile.lfy:compile#compile:compile:a65958efbe042be49d8dfc6e93e58d1f6f3b8ea94ee65ea9df5a2c8e2d1ea43f
    // @lfy def/cli/compile.lfy:compile#compile:compile:2980264ce3d1cc374ba5efe5189f297ea1edce70b8a4cffbc8e194f056d2131b
    // @lfy def/cli/compile.lfy:compile#compile:compile:e53fbd4d8f8cab911011548853cc6a5424f1956917fc2d11c49399ff0acaa3cc
    #[expect(clippy::too_many_arguments, reason = "one verification of a unit takes all of this")]
    pub(crate) fn verify_unit(
        &mut self,
        unit: usize,
        command: &str,
        directory: &Path,
        maps: &[SourceMap],
        records: &[ReviewRecord],
        runs: &Runs,
        resuming: bool,
    ) -> Verified {
        let Some(verifier) = self.verifier.clone() else {
            return Verified::Passed;
        };
        let _ = command;
        let verification = self.review_unit(unit, &verifier, directory, true, maps, records, runs, resuming);
        // What a stopped run gives is ignored.
        // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
        if self.halted {
            return Verified::Stopped;
        }
        let report = &verification.report;
        let carried = verification.request.carried.len();
        let stem = self.stem(unit);
        // One reviewed line follows, counting the reviews of this run, and then the verdicts
        // carried when there are any, or ending with the unverifiable when there are none.
        // @lfy def/cli/compile.lfy:compile#compile:compile:a65958efbe042be49d8dfc6e93e58d1f6f3b8ea94ee65ea9df5a2c8e2d1ea43f
        // @lfy def/cli/compile.lfy:compile#compile:compile:0ae375c1084691369dda163c81e2e1f335de4780dfb63290fe31e727ca79fec3
        // @lfy def/cli/compile.lfy:compile#compile:compile:79aeca787363a11f87e229a581c29dfcab7548d25cd5e7e3018983a54dbcb14d
        // @lfy def/cli/compile.lfy:compile#compile:compile:42cb7931abf1b1cbc69474e94e4981f4280630610c6622305d2f9ce3dab316d1
        // @lfy def/cli/compile.lfy:compile#compile:compile:14cbfe6603ade5c97b980e258fee2c6beaff3dce14dd1ea1d7e29c34507da39f
        let counts = counts_with_carried(&report.reviews, report.missing.len(), carried);
        // The reviewed line and everything that follows it are printed in one turn, so no line
        // of another unit comes between them.
        // @lfy def/cli/compile.lfy:compile#compile:compile:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
        let _turn = self.reporter.hold();
        self.reporter.report_counts(Step::Reviewed, Some(&stem), &counts, None);
        // Nothing was asked: nothing was reviewed, so nothing was violated either.
        // @lfy def/cli/compile.lfy:compile#compile:compile:14cbfe6603ade5c97b980e258fee2c6beaff3dce14dd1ea1d7e29c34507da39f
        if verification.request.asked.is_empty() {
            return Verified::Passed;
        }
        // Each id no review names is named as not reviewed, on one line under the reviewed
        // line, and counts as unverifiable above.
        // @lfy def/cli/compile.lfy:compile#compile:compile:97ca8a3e66805eba7104729c205da94cf64474677a3c53e056be523758150d9d
        if !report.missing.is_empty() {
            self.reporter.reason(&missing_lines(report));
        }
        // A run that stood had anything it wrote beside its reviews printed under its
        // reviewed line, and is not run again for it; a run that ended without every verdict
        // had its problems printed as a failed line already.
        // @lfy def/cli/compile.lfy:compile#compile:compile:5a007ec86d1ce702345b9b34c99744c4d8993ccab14f803a47d7740f0c6efc9c
        // @lfy def/cli/compile.lfy:compile#compile:compile:7fbd8e92981ac0bfb3a6651b7e7bf907ebdd945ef7a5649a3b4a8be0fe8c6fcb
        if !verification.failed {
            self.reporter.problems(&report.problems);
        }
        // An unverifiable review is counted in the reviewed line and never rejects the unit.
        // @lfy def/cli/compile.lfy:compile#compile:compile:2980264ce3d1cc374ba5efe5189f297ea1edce70b8a4cffbc8e194f056d2131b
        // @lfy def/cli/compile.lfy:compile#compile:compile:9854e85deefe68148920771ac99fcfeab835dc084af3382a76328d6730b88e94
        let problems = violations_of(report);
        if problems.is_empty() {
            return Verified::Passed;
        }
        // The problems are printed: one per violated review, reading failure at, the file, a
        // colon, the line, a colon, the note, and the evidence in parentheses, and one per
        // unplaced violation.
        // @lfy def/cli/compile.lfy:compile#compile:compile:836ff964045c3fdf6d9f590423c540dcc1814918a94ca6164f5840c078d87342
        // @lfy def/cli/compile.lfy:compile#compile:compile:8533b0690caaa0d5a3703e6633b44fd6034ce112c1a2c32f151e384e973b8bdf
        // @lfy def/cli/compile.lfy:compile#compile:compile:6c97f065ec22808dd6a71afa10fbb1b9f2c63c5fe6ab869aeeb129eeb68a59d1
        self.report_rejection(unit, &problems);
        // The review made once more carries what this one settled; an unplaced violation
        // names no criterion, so nothing of this review can stand and the next one carries
        // nothing at all.
        // @lfy def/cli/compile.lfy:compile#compile:compile:c3711c547c11411e55264eed71019f21008660209ec5348f9c940bbad6ddeb23
        let next = if report.unplaced.is_empty() {
            Run::carried_on(records, &verification.request, &report.reviews, false)
        } else {
            Vec::new()
        };
        Verified::Violated { problems, next }
    }

    /// Every global criterion and test reviewed once, in a directory, against the source maps
    /// given: one globalVerifying progress line, the verifier run, the review written to
    /// `elfie-requests/global.reviews.json` under the root, and one globalReviewed line.
    /// Gives the report, or nothing when the program has no global criterion and no global
    /// test or no verifier is named.
    // @lfy def/cli/compile.lfy:compile#compile:compile:0b09728c30bb0edb3d089358237c8d4a4b44e18393eb5824b8af1001750100e0
    // @lfy def/cli/compile.lfy:compile#compile:compile:67b4fcdb83c0d4fb7760fbaab79b47962fe177863768224d0870c81726f1cc08
    // @lfy def/cli/compile.lfy:compile#compile:compile:5ba968cf4e110e45b045c0bebeafc56f21cb68fcd6d5e2652f7155b49bb57385
    pub(crate) fn global_review(&mut self, directory: &Path, maps: &[SourceMap]) -> Option<ReviewReport> {
        // No global criterion and no global test: no global review runs and no file is
        // written.
        // @lfy def/cli/compile.lfy:compile#compile:compile:f033aae0b13c3e667e25d9d067a7b58b8f438e3b2854dcb127d56baca1010a38
        if self.program.criteria.is_empty() && self.program.tests.is_empty() {
            return None;
        }
        let command = self.verifier.clone()?;
        // @lfy def/cli/compile.lfy:compile#compile:compile:0b09728c30bb0edb3d089358237c8d4a4b44e18393eb5824b8af1001750100e0
        self.reporter.report(Step::GlobalVerifying, None, &command);
        let requirements = GlobalReview::requirements_of(&self.program);
        // Its input is the global review request of the plan and every source map given.
        // @lfy def/cli/compile.lfy:compile#compile:compile:67b4fcdb83c0d4fb7760fbaab79b47962fe177863768224d0870c81726f1cc08
        let request = generation::global_review(&self.program, maps);
        let report = self.ask_globally(&command, directory, &request, maps)?;
        let root = self.root();
        self.write_global_reviews(&root, &requirements, &report);
        // @lfy def/cli/compile.lfy:compile#compile:compile:5ba968cf4e110e45b045c0bebeafc56f21cb68fcd6d5e2652f7155b49bb57385
        let counts = counts_with_missing(&report.reviews, report.missing.len());
        self.reporter.report_counts(Step::GlobalReviewed, None, &counts, None);
        if !report.missing.is_empty() {
            self.reporter.reason(&missing_lines(&report));
        }
        // One problem per violated review: failure at, a space, global, a colon, a space, the
        // note, and the evidence in parentheses.
        // @lfy def/cli/compile.lfy:compile#compile:compile:4df83fe74548903b3edfa0c021389b44e555ff2b79a5e259ea30564dbc9dca2c
        let problems: Vec<String> = report
            .reviews
            .iter()
            .filter(|review| review.status == ReviewStatus::Violated)
            .map(|review| format!("failure at {GLOBAL}: {} ({})", review.note, review.evidence))
            .collect();
        self.reporter.problems(&problems);
        Some(report)
    }

    /// `elfie-requests/global.reviews.json` under the root written with the SHA-256 of the
    /// global ids reviewed and the report, replacing an earlier file.
    // @lfy def/cli/compile.lfy:compile#compile:compile:033aa06dcbf3008dd1606e0361145d7bd0386bb1efe3b055336c5e754f6c7eeb
    pub(crate) fn write_global_reviews(&mut self, root: &Path, requirements: &str, report: &ReviewReport) {
        // A GlobalReview holding that report and the SHA-256 of the global ids reviewed,
        // written where the agent server reads it from.
        // @lfy def/cli/compile.lfy:compile#compile:compile:033aa06dcbf3008dd1606e0361145d7bd0386bb1efe3b055336c5e754f6c7eeb
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

    /// The verifier run once for the global review, and its report: the global
    /// review request as its input and the directory given as its directory, with one hosted
    /// session bound to it, no unit, and the source maps that request was made with.
    ///
    /// Nothing when the agent server could not be hosted: its failure is printed, the code is
    /// failure, and nothing more runs, so no review is read or written.
    // @lfy def/cli/compile.lfy:compile#compile:compile:eae2a79c9a6dc51a5daba684245211bdf83b66ea0d0b34fbdc86bb68df82a42d
    // @lfy def/cli/compile.lfy:verify#verify:verify:32387edd44a4cc7c91efe2a37b89320c59bfef52abddb28e5661c9df8f68b514
    // @lfy def/cli/compile.lfy:compile#compile:compile:73c389b755f64b116aefe36d13a3dbbe224b401808c21b5c6e58de07a4b1f1d6
    // @lfy def/cli/compile.lfy:compile#compile:compile:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
    pub(crate) fn ask_globally(
        &mut self,
        command: &str,
        directory: &Path,
        request: &ReviewRequest,
        maps: &[SourceMap],
    ) -> Option<ReviewReport> {
        // A run for the global review is no resuming run.
        // @lfy def/cli/compile.lfy:compile#compile:compile:9e092cdf07f0cd7a1468317b2e75bb10ee5513015372fa964ca1fdff2d436dba
        let runs = Runs::new(self.agent.clone(), self.running.clone());
        // @lfy def/cli/compile.lfy:compile#compile:compile:eae2a79c9a6dc51a5daba684245211bdf83b66ea0d0b34fbdc86bb68df82a42d
        let binding = self.binding_of(Role::Verifier, None, directory, None, None, Some(request.clone()), maps.to_vec());
        // @lfy def/cli/compile.lfy:compile#compile:compile:73c389b755f64b116aefe36d13a3dbbe224b401808c21b5c6e58de07a4b1f1d6
        let exit = runs.run(command, directory, GLOBAL, &request.instructions, binding, false);
        // The agent server could not be hosted: its failure is printed, no verifier runs
        // after it, and the code is failure.
        // @lfy def/cli/compile.lfy:compile#compile:compile:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
        if self.agent.server.failed() {
            self.hosting_failed(None, exit.stderr.trim());
            return None;
        }
        // The verifier runs once for the global review; a report that ended without every
        // verdict stands as it is, and what it misses is said.
        let (report, failed) = self.report_of(&exit, &request.asked);
        if failed {
            self.note_incomplete(GLOBAL, &report, exit.code);
        }
        Some(report)
    }

    /// The agent server could not be hosted: its failure is printed, as a failed progress line
    /// and to standard error, the code is failure, and the command stops, so no compiler or
    /// verifier runs after it.
    // @lfy def/cli/compile.lfy:compile#compile:compile:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
    pub(crate) fn hosting_failed(&mut self, unit: Option<&str>, message: &str) {
        self.reporter.report_problems(Step::Failed, unit, message, &[message.to_string()]);
        complain(self.style.err, None, message);
        self.worsen(ExitCode::Failure);
        self.halted = true;
    }

    /// The stems of every unit whose source maps, among those given, hold a marker whose
    /// requirement the test accepts.
    // @lfy def/cli/compile.lfy:compile#compile:compile:1435376dfcd2c7ecb8f1966cb1ce291a33eaded750ea02d6c8b936baa000130c
    pub(crate) fn stems_answering_with(&self, maps: &[SourceMap], answers: impl Fn(&str) -> bool) -> Vec<String> {
        let mut stems: Vec<String> = Vec::new();
        for unit in &self.plan.units {
            let target = &self.program.workspace.targets[unit.target].identifier;
            let source = &self.program.workspace.files[unit.file].path;
            // @lfy def/cli/compile.lfy:compile#compile:compile:1435376dfcd2c7ecb8f1966cb1ce291a33eaded750ea02d6c8b936baa000130c
            let answered = maps
                .iter()
                .filter(|map| &map.target == target && &map.source == source)
                .any(|map| map.markers.iter().any(|marker| marker.requirement.as_deref().is_some_and(&answers)));
            if answered {
                stems.push(unit.stem.clone());
            }
        }
        stems.sort();
        stems.dedup();
        stems
    }
}

/// What asking the verifier came to: its report, whether the run that gave it ended without
/// every verdict, so that the problems of such a run are not printed twice, and the review
/// request it was given, which says what it was asked about and what it carried.
// @lfy def/cli/compile.lfy:compile#compile:compile:0ae375c1084691369dda163c81e2e1f335de4780dfb63290fe31e727ca79fec3
#[derive(Default)]
pub(crate) struct Verification {
    pub(crate) report: ReviewReport,
    pub(crate) failed: bool,
    /// The review request the run was given, or the one that asked nothing.
    pub(crate) request: ReviewRequest,
}

/// Whether a run of the verifier ended without every verdict rather than merely writing
/// something beside its reviews: it held no review at all, or it left an id it was asked
/// about unanswered and either wrote no line reading `ELFIE: REVIEWED` or ended by a signal
/// or could not be started.
///
/// A run that answered for every id it was asked about stands as it is, whatever prose
/// surrounds the answers and however it ended, and so does one that wrote the end line and
/// ended well with some of them left over: what it left is counted as unverifiable.
// @lfy def/cli/compile.lfy:compile#compile:compile:dbcb80625881c43a2f36b95af5c8519113c0f1736c3c3171e2e3f55e426bd6d7
// @lfy def/cli/compile.lfy:compile#compile:compile:7fbd8e92981ac0bfb3a6651b7e7bf907ebdd945ef7a5649a3b4a8be0fe8c6fcb
pub(crate) fn verifier_failed(exit: &Exit, report: &ReviewReport) -> bool {
    if report.reviews.is_empty() {
        return true;
    }
    let ended = exit.code != -1 && exit.stdout.lines().any(|line| line.trim() == REVIEWED);
    !report.missing.is_empty() && !ended
}

/// The diagnostics a verify prints: the error ones alone.
// @lfy def/cli/compile.lfy:verify#verify:verify:83640c302c4ea1762f8e4981643b58c9c5094bba9180dc2995305d2406a7615c
pub(crate) fn error_diagnostics(workspace: &Workspace) -> Vec<Diagnostic> {
    query::diagnostics_of(workspace, None).into_iter().filter(|d| d.severity == Severity::Error).collect()
}

/// Every error diagnostic of the program printed; whether there was one.
// @lfy def/cli/compile.lfy:verify#verify:verify:83640c302c4ea1762f8e4981643b58c9c5094bba9180dc2995305d2406a7615c
pub(crate) fn print_errors(workspace: &Workspace, json: bool, on: bool) -> bool {
    let errors = error_diagnostics(workspace);
    for diagnostic in &errors {
        print_diagnostic(diagnostic, json, on);
    }
    !errors.is_empty()
}

/// Verify is compile without the compiler: the same plan, the same review request, the same
/// verifier, so a person can ask for an opinion on outputs already recorded. Nothing is
/// compiled, no output is written, and nothing under elfie-compile is recorded, moved, or
/// removed; what relocating the outputs gives is read and never written.
// @lfy def/cli/compile.lfy:verify#verify:verify:64bc2fc6236a9783f1939c6823437a0fee24fced7f789dc76b7dd418a1260f99
// @lfy def/cli/compile.lfy:verify#verify:verify:57c501f4ac1d2297555bfaa08f4fb4c51e6c4d22db2a4b047c43c8a3e2c631e2
// @lfy def/cli/compile.lfy:verify#verify:verify:abb5794d626f24032c066316c918b8b21a401336dac8fa0f1f415ec179aabd6a
pub(crate) fn verify(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    let workspace = workspace::load(root);
    let json = invocation.flag("json");
    let style = Paint::of(invocation);
    // Every error diagnostic is printed and verify ends with the code problems.
    // @lfy def/cli/compile.lfy:verify#verify:verify:83640c302c4ea1762f8e4981643b58c9c5094bba9180dc2995305d2406a7615c
    if print_errors(&workspace, json, style.out) {
        // @lfy def/cli/compile.lfy:verify#verify:verify:cd777dab37a66b80b804ad0e2c1c6eb305a85f5c6f365759da5469e81ffe4dbf
        return ExitCode::Problems;
    }
    let target = match target_named(&workspace, invocation) {
        Ok(target) => target,
        Err(code) => return code,
    };
    // The outputs reviewed are those of the source maps relocating the plan made from the
    // source maps of the workspace gives, with the root as its directory; nothing it gives is
    // written, and every plan verify makes is made from those maps.
    // @lfy def/cli/compile.lfy:verify#verify:verify:57c501f4ac1d2297555bfaa08f4fb4c51e6c4d22db2a4b047c43c8a3e2c631e2
    // @lfy def/cli/compile.lfy:verify#verify:verify:abb5794d626f24032c066316c918b8b21a401336dac8fa0f1f415ec179aabd6a
    let recorded = generation::source_maps_of(&workspace, target.as_deref());
    drop(workspace);
    let (first, before) = plan_with(root, &recorded, &[], &[], &[]);
    let relocated =
        crate::compiling::scoped(generation::relocate(&first, &before, Some(root)), &first.workspace, target.as_deref());
    drop((first, before));
    let maps = relocated.source_maps.clone();
    // Plan.problems is neither printed nor counted in the exit code here, and the review goes
    // on: a person asked for an opinion on what is recorded, not for a build.
    // @lfy def/cli/compile.lfy:verify#verify:verify:304ed515dfa3121d80f29c65da75d99d67be3708e1f5c3c46961a2dbbbc46322
    let (_, known) = plan_with(root, &maps, &[], &[], &[]);
    // Every stem given as a requested unit or, with no stem, every unit whose outputs are
    // not empty.
    // @lfy def/cli/compile.lfy:verify#verify:verify:c98c75ccd7e9b5e3cb007cb35cb2628fd4d93779a93201868a8bf2df0c6ac604
    // @lfy def/cli/compile.lfy:verify#verify:verify:d665f83cc9e0222f6d5e72fcdc1506878985d1e586251c478035d704af123680
    let mut requested: Vec<String> = Vec::new();
    // --global with no stem reviews no unit.
    // @lfy def/cli/compile.lfy:verify#verify:verify:2807195e43f8abc32d92c01234487814a095565d2b9a42d8da80b6f24a665bb6
    let global_only = invocation.flag("global") && invocation.arguments.is_empty();
    if invocation.arguments.is_empty() {
        // @lfy def/cli/compile.lfy:verify#verify:verify:d665f83cc9e0222f6d5e72fcdc1506878985d1e586251c478035d704af123680
        requested.extend(known.units.iter().filter(|unit| !unit.outputs.is_empty()).map(|unit| unit.stem.clone()));
    } else {
        // @lfy def/cli/compile.lfy:verify#verify:verify:c98c75ccd7e9b5e3cb007cb35cb2628fd4d93779a93201868a8bf2df0c6ac604
        for stem in &invocation.arguments {
            // A stem naming a unit whose outputs are empty is printed as having nothing to
            // review and left out, and the code does not change for it.
            // @lfy def/cli/compile.lfy:verify#verify:verify:f6ecc4facc68022e7002a7e92ab9ba501e6cdd6242d9d87adb8b05ae09c3533f
            // @lfy def/cli/compile.lfy:verify#verify:verify:62de602001e5a8cd6c08fe0b373ce392f7dfbc93fe340e17701e10e49d405d69
            match known.units.iter().find(|unit| &unit.stem == stem) {
                Some(unit) if !unit.outputs.is_empty() => requested.push(stem.clone()),
                _ if json => println!("{}", serde_json::json!({ "stem": stem, "message": "nothing to review" })),
                _ => println!("{stem}: nothing to review"),
            }
        }
    }
    // After any units are reviewed, every global criterion and test is reviewed once with no
    // stem given, or with --global.
    // @lfy def/cli/compile.lfy:verify#verify:verify:d91e473bb0065ff6b530621d2d948f943c39c400da2fd41ae66086d11c34d2cd
    let globally = invocation.flag("global") || invocation.arguments.is_empty();
    if requested.is_empty() && !globally {
        if !json {
            println!("nothing to review");
        }
        return ExitCode::Success;
    }
    let planning = Planning { maps: maps.clone(), requested: requested.clone(), violated: Vec::new() };
    let (program, plan) = planning.plan(root, &[]);
    let wanted: BTreeSet<&str> = requested.iter().map(String::as_str).collect();
    // Each requested unit, in plan order, of the target --target names when it is given.
    // @lfy def/cli/compile.lfy:verify#verify:verify:5ae62b936ea6ca73ed12ba186677c8460636fef0ec96946d8e147e49a205d24f
    // @lfy def/cli/compile.lfy:verify#verify:verify:aa8d8f7253225d4bbfb5d6995399aa58057a616f7d55984c01a6687dc0ff2ee9
    let units: Vec<usize> = if global_only {
        Vec::new()
    } else {
        (0..plan.units.len())
            .filter(|&index| {
                let unit = &plan.units[index];
                target.as_deref().is_none_or(|name| target_of(&program.workspace, unit) == name)
                    && wanted.contains(unit.stem.as_str())
            })
            .collect()
    };
    let reporter = Reporter::new(json, style, units.len(), log_paths(root));
    let mut run = Run::new(program, plan, planning, reporter, invocation);
    // @lfy def/cli/compile.lfy:verify#verify:verify:62ee8e26855d189d744062b262cfed91d039f0cd61f31e843738d7a60ec73d44
    run.relocation = relocated;
    // Units are reviewed one at a time, in plan order, in the root.
    // @lfy def/cli/compile.lfy:verify#verify:verify:5ae62b936ea6ca73ed12ba186677c8460636fef0ec96946d8e147e49a205d24f
    for unit in units {
        match run.verifier.clone() {
            // The verifier runs on each requested unit whose review request asks something:
            // one stream of the verifier command, with ReviewRequest.instructions of the
            // unit's review request as its input and the root as its directory.
            // @lfy def/cli/compile.lfy:verify#verify:verify:1039b4b7406b178e04310e0fd85a27f9e235426c129ac1656a155bd2e4034d30
            // @lfy def/cli/compile.lfy:verify#verify:verify:4185b35d14b24c85bfd17b51dfbe20df36cada069c9fb42bf4e35263334a5d97
            Some(command) => {
                let root = run.root();
                // What the unit's review carries: what verify's relocation moved for it, or
                // what it was last reviewed as, and nothing at all with --full.
                // @lfy def/cli/compile.lfy:verify#verify:verify:62ee8e26855d189d744062b262cfed91d039f0cd61f31e843738d7a60ec73d44
                // @lfy def/cli/compile.lfy:verify#verify:verify:9fc0da16b4fb4241e8c2e859851d9e3fb85f014aa0a2f7a997decf40c5da9d86
                let records = run.verify_records(unit, &root);
                let recorded = run.maps.clone();
                // A verify's run of the verifier is a first run, so it goes on from nothing.
                // @lfy def/cli/compile.lfy:compile#compile:compile:9e092cdf07f0cd7a1468317b2e75bb10ee5513015372fa964ca1fdff2d436dba
                let runs = Runs::new(run.agent.clone(), run.running.clone());
                let verification =
                    run.review_unit(unit, &command, &root, false, &recorded, &records, &runs, false);
                let carried = verification.request.carried.len();
                let report = verification.report;
                // Every review printed, then the counts, then the id of every criterion and
                // test the review left unanswered. A review request that asked nothing ran no
                // verifier, so it prints no review and counts none.
                // @lfy def/cli/compile.lfy:verify#verify:verify:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
                // @lfy def/cli/compile.lfy:verify#verify:verify:1210eb92f28da423c86881acae7b44dcc7eb7346006e861a85cef47a910c5f8f
                // @lfy def/cli/compile.lfy:verify#verify:verify:2edbac225cec2318ff261227fbb09c7256786df4743f9748be8742962271a3ac
                print_reviews(&report.reviews, &report.missing, carried, run.json, run.style.out);
                // @lfy def/cli/compile.lfy:verify#verify:verify:e66e42fc4344ab7898da31692b13cfa9d1621e6c695a0226fd0e42bb5522bdd4
                if counts_of(&report.reviews).1 > 0 {
                    run.worsen(ExitCode::Problems);
                }
            }
            // With no verifier named, the review request of each requested unit is written
            // for a verifier run by hand, and the code is success.
            // @lfy def/cli/compile.lfy:verify#verify:verify:4433e487958c0421f6e9121d7dec547034bf56c187a29d2478a9f228caba4eb4
            // @lfy def/cli/compile.lfy:verify#verify:verify:b6938589b155e207c8b6d2d220a8d8726be7f451d3867f2a1d037c89951d9923
            None => run.write_review_request(unit),
        }
    }
    // @lfy def/cli/compile.lfy:verify#verify:verify:d91e473bb0065ff6b530621d2d948f943c39c400da2fd41ae66086d11c34d2cd
    if globally && run.verifier.is_some() {
        // Nothing of the compile's own gating applies here: a person asked for the opinion.
        // The run is one stream of the verifier, with the global review request of the plan
        // and the source maps as its input and the root as its directory.
        // @lfy def/cli/compile.lfy:verify#verify:verify:32387edd44a4cc7c91efe2a37b89320c59bfef52abddb28e5661c9df8f68b514
        // An agent server that could not be hosted has had its failure printed and made the
        // code failure, and gives no report to print.
        // @lfy def/cli/compile.lfy:compile#compile:compile:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
        if let Some(report) = verify_globally(&mut run) {
            // A global review carries nothing: every global criterion and test is reviewed
            // once. Each id no review names is named as not reviewed and counts as
            // unverifiable.
            // @lfy def/cli/compile.lfy:compile#compile:compile:97ca8a3e66805eba7104729c205da94cf64474677a3c53e056be523758150d9d
            print_reviews(&report.reviews, &report.missing, 0, json, style.out);
            // @lfy def/cli/compile.lfy:verify#verify:verify:e66e42fc4344ab7898da31692b13cfa9d1621e6c695a0226fd0e42bb5522bdd4
            if counts_of(&report.reviews).1 > 0 {
                run.worsen(ExitCode::Problems);
            }
        }
    }
    // Every session of this verify is ended and the server stopped, after the last run has
    // returned and before the command returns, whatever the outcome.
    // @lfy def/cli/compile.lfy:compile#compile:compile:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
    run.agent.server.stop();
    // No review is violated and the verifier was started, or no review request asked
    // anything: the code is success; it could not be started: the code is failure.
    // @lfy def/cli/compile.lfy:verify#verify:verify:c3a2ad3cc8b8443eed86ed501261573e2799abb00383db190b158541be935033
    // @lfy def/cli/compile.lfy:verify#verify:verify:f4d733d1c98fcb52efb7af2bcf5a54e5df115f3454b8c0ce42aa336de6dd2857
    run.code
}

/// Every global criterion and test reviewed once for the verify command, in the root, with
/// the recorded source maps; the review is written as a compile writes one.
// @lfy def/cli/compile.lfy:verify#verify:verify:32387edd44a4cc7c91efe2a37b89320c59bfef52abddb28e5661c9df8f68b514
pub(crate) fn verify_globally(run: &mut Run) -> Option<ReviewReport> {
    // No global criterion and no global test: no global review runs and no file is
    // written, here as in a compile.
    // @lfy def/cli/compile.lfy:compile#compile:compile:f033aae0b13c3e667e25d9d067a7b58b8f438e3b2854dcb127d56baca1010a38
    if run.program.criteria.is_empty() && run.program.tests.is_empty() {
        return Some(ReviewReport::default());
    }
    let command = run.verifier.clone()?;
    let root = run.root();
    let maps = run.maps.clone();
    // @lfy def/cli/compile.lfy:verify#verify:verify:32387edd44a4cc7c91efe2a37b89320c59bfef52abddb28e5661c9df8f68b514
    let request = generation::global_review(&run.program, &maps);
    let requirements = GlobalReview::requirements_of(&run.program);
    // An agent server that could not be hosted gives no report, and nothing is written.
    // @lfy def/cli/compile.lfy:compile#compile:compile:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
    let report = run.ask_globally(&command, &root, &request, &maps)?;
    // @lfy def/cli/compile.lfy:compile#compile:compile:033aa06dcbf3008dd1606e0361145d7bd0386bb1efe3b055336c5e754f6c7eeb
    run.write_global_reviews(&root, &requirements, &report);
    Some(report)
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
// @lfy def/cli/compile.lfy:verify#verify:verify:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
// @lfy def/cli/compile.lfy:verify#verify:verify:aefab83034ace05b16e3a0289b1a6d708bca8805648197925ce1c4cb013db13d
// @lfy def/cli/compile.lfy:verify#verify:verify:2edbac225cec2318ff261227fbb09c7256786df4743f9748be8742962271a3ac
pub(crate) fn print_reviews(reviews: &[Review], unanswered: &[String], carried: usize, json: bool, on: bool) {
    let missing = unanswered.len();
    for review in reviews {
        // @lfy def/cli/compile.lfy:verify#verify:verify:639501265f62b510499fe150b611345c387104c04de980656103153401de8d17
        if json {
            println!("{}", review.to_json());
        } else {
            // @lfy def/cli/compile.lfy:verify#verify:verify:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
            println!("{}", painted_review(review, on));
        }
    }
    // After each unit one line gives the counts of satisfied, violated, and unverifiable,
    // every id no review names counted among the unverifiable.
    // @lfy def/cli/compile.lfy:verify#verify:verify:aefab83034ace05b16e3a0289b1a6d708bca8805648197925ce1c4cb013db13d
    let counts = counts_of(reviews);
    if json {
        println!(
            "{}",
            serde_json::json!({
                "satisfied": counts.0,
                "violated": counts.1,
                "unverifiable": counts.2 + missing,
                // @lfy def/cli/compile.lfy:verify#verify:verify:e16c3a64811e996470b960cf5e81ef245970ea83b63e901e2b4707167e69d564
                "carried": carried,
            })
        );
    } else {
        // The counts line begins with the satisfied, the violated, and the unverifiable, and
        // ends with the verdicts the review request carried when it carried any.
        // @lfy def/cli/compile.lfy:verify#verify:verify:4fe542a448b7e8a76be032645d2c1ec9403ff406a3b0b02a86b91b8e4a883140
        // @lfy def/cli/compile.lfy:verify#verify:verify:e16c3a64811e996470b960cf5e81ef245970ea83b63e901e2b4707167e69d564
        println!("{}", counts_message(&counts_with_carried(reviews, missing, carried), on));
        // Each id no review names is named as not reviewed, on one line under the counts
        // line, and counts as unverifiable above.
        // @lfy def/cli/compile.lfy:compile#compile:compile:97ca8a3e66805eba7104729c205da94cf64474677a3c53e056be523758150d9d
        if !unanswered.is_empty() {
            println!("{INDENT}{}", paint(&not_reviewed(unanswered), Tone::Warning, on));
        }
    }
}

/// One review as the verify command prints it, painted.
// @lfy def/cli/compile.lfy:verify#verify:verify:0011d65e2e66e2fa6bffcf28bb68b59c7d3425bddda6fd61a196e714724e5195
pub(crate) fn painted_review(review: &Review, on: bool) -> String {
    // The status is padded to the longest status of ReviewStatus.
    // @lfy def/cli/compile.lfy:verify#verify:verify:0011d65e2e66e2fa6bffcf28bb68b59c7d3425bddda6fd61a196e714724e5195
    let column = [ReviewStatus::Satisfied, ReviewStatus::Violated, ReviewStatus::Unverifiable]
        .iter()
        .map(|status| width(&status.to_string()))
        .max()
        .unwrap_or(0);
    // The status is painted in its own tone, the file and line are subject, and the entity
    // and the note are plain.
    // @lfy def/cli/compile.lfy:verify#verify:verify:0011d65e2e66e2fa6bffcf28bb68b59c7d3425bddda6fd61a196e714724e5195
    let status = padded(&paint(&review.status.to_string(), status_tone(review.status), on), column);
    format!(
        "{status} {} {}: {}",
        paint(&format!("{}:{}", review.file, review.line), Tone::Subject, on),
        review.entity,
        review.note
    )
}
