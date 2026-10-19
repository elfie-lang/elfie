// @lfy def/cli/main.lfy:main
//! What one compile holds while it runs: the plan and the source maps, the change set of each
//! target, the outputs a unit owns in its directory, a unit's previous source, the request
//! made for each unit, accepting and recording what a unit earned, and the gate a unit's
//! outputs pass before they are accepted.

use super::*;
use crate::processes::{
    Agent, Ran, Running, complain_logged, line_for, locations_of, run_target_command, within, word_of_operation,
};

/// The files under the target's output directory, in the directory a unit runs in, split into
/// the ones that carry a marker for the unit, as [`own_marker`] reads one, and every other one
/// of that listing, every directory named target, .git, or node_modules skipped. Their paths
/// are relative to that directory, which is the root or a change set's copy of it, so an
/// output is named the same either way.
///
/// The second list is what `accept` is given as its `elsewhere`: a file of it earns no source
/// map and none of its markers is checked, and it is there so that a region this unit moved
/// out of a shared file into a file of its own is still found where it went.
// @lfy def/cli/main.lfy:main#main:main:91ffd2f6eafdd00f74f9012574d3c7ae2bd6569a8b89e66b9bfb6b117a6c8488
// @lfy def/cli/main.lfy:main#main:main:c3dec788ff5d654d7dbac2d0021b2e65e093abd3a174484ae34c58ff953c779a
pub(crate) fn outputs_of(
    workspace: &Workspace,
    plan: &Plan,
    unit: usize,
    directory: &Path,
) -> (Vec<Output>, Vec<Output>) {
    let target = &workspace.targets[plan.units[unit].target];
    let mut found = Vec::new();
    let mut elsewhere = Vec::new();
    // @lfy def/cli/main.lfy:main#main:main:91ffd2f6eafdd00f74f9012574d3c7ae2bd6569a8b89e66b9bfb6b117a6c8488
    for output in files_in(target, directory) {
        // A marker for the unit names the unit's file, or an entity moved into the unit and
        // the file that declares it.
        // @lfy def/cli/main.lfy:main#main:main:c3dec788ff5d654d7dbac2d0021b2e65e093abd3a174484ae34c58ff953c779a
        if generation::parse_markers(&output.text).iter().any(|m| own_marker(workspace, plan, unit, m)) {
            found.push(output);
        } else {
            // @lfy def/cli/main.lfy:main#main:main:91ffd2f6eafdd00f74f9012574d3c7ae2bd6569a8b89e66b9bfb6b117a6c8488
            elsewhere.push(output);
        }
    }
    (found, elsewhere)
}

/// Every file of a target's output directory in a directory, recursively skipping every
/// directory named target, .git, or node_modules, as it reads there, by its path relative to
/// the directory, in path order. A file that does not read as text is left out.
// @lfy def/cli/main.lfy:main#main:main:7d2892039e629efc80e06fa9613480a7affbe990c13c5aeb53877f888c21b969
pub(crate) fn files_in(target: &Target, directory: &Path) -> Vec<Output> {
    let mut paths = Vec::new();
    // @lfy def/cli/main.lfy:main#main:main:7d2892039e629efc80e06fa9613480a7affbe990c13c5aeb53877f888c21b969
    walk_all(&directory.join(&target.output_directory), &mut paths);
    paths.sort();
    paths
        .iter()
        .filter_map(|path| {
            let text = fs::read_to_string(path).ok()?;
            Some(Output { path: relative(directory, path), text })
        })
        .collect()
}

/// Whether a marker is one unit's own: it names the unit's file, or it names an entity moved
/// into the unit and the file that declares it.
// @lfy def/cli/main.lfy:main#main:main:3c7ca0bb8036d2eeb1a0e1dcf624fcab14d99d0f692a49c73b4138cb385609af
pub(crate) fn own_marker(workspace: &Workspace, plan: &Plan, unit: usize, marker: &Marker) -> bool {
    // @lfy def/cli/main.lfy:main#main:main:3c7ca0bb8036d2eeb1a0e1dcf624fcab14d99d0f692a49c73b4138cb385609af
    if marker.file == workspace.files[plan.units[unit].file].path {
        return true;
    }
    // @lfy def/cli/main.lfy:main#main:main:3c7ca0bb8036d2eeb1a0e1dcf624fcab14d99d0f692a49c73b4138cb385609af
    plan.moves
        .iter()
        .filter(|moved| moved.into == unit)
        .filter(|moved| workspace.files[plan.units[moved.origin].file].path == marker.file)
        .flat_map(|moved| moved.entities.iter())
        .any(|&entity| {
            let Some(name) = workspace.model.entities[entity].identifier.as_deref() else {
                return false;
            };
            marker.entity.as_deref().is_some_and(|spelled| {
                spelled == name || spelled.strip_prefix(name).is_some_and(|rest| rest.starts_with('.'))
            })
        })
}

/// The unit's own region text of one output, as a source map defines it: the text of the
/// regions its own markers begin, in output order, each line with the marker comment it
/// carries and the white space before it taken off and every line that is then empty left
/// out.
// @lfy def/cli/main.lfy:main#main:main:3c7ca0bb8036d2eeb1a0e1dcf624fcab14d99d0f692a49c73b4138cb385609af
pub(crate) fn own_region_text(workspace: &Workspace, plan: &Plan, unit: usize, text: &str) -> String {
    let comment = &workspace.targets[plan.units[unit].target].marker_comment;
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    // @lfy def/cli/main.lfy:main#main:main:3c7ca0bb8036d2eeb1a0e1dcf624fcab14d99d0f692a49c73b4138cb385609af
    for marker in generation::parse_markers(text) {
        if !own_marker(workspace, plan, unit, &marker) {
            continue;
        }
        let last = marker.end.min(lines.len());
        let first = marker.output_line.saturating_sub(1).min(last);
        for line in &lines[first..last] {
            // @lfy def/cli/main.lfy:main#main:main:3c7ca0bb8036d2eeb1a0e1dcf624fcab14d99d0f692a49c73b4138cb385609af
            let kept = without_marker(line, comment);
            if kept.is_empty() {
                continue;
            }
            out.push_str(kept);
            out.push('\n');
        }
    }
    out
}

/// One line of an output with the marker comment it carries, and the white space before it,
/// taken off; the line as it is when it carries none.
// @lfy def/cli/main.lfy:main#main:main:3c7ca0bb8036d2eeb1a0e1dcf624fcab14d99d0f692a49c73b4138cb385609af
pub(crate) fn without_marker<'l>(line: &'l str, comment: &str) -> &'l str {
    if comment.is_empty() {
        return line;
    }
    let mut from = 0;
    // @lfy def/cli/main.lfy:main#main:main:3c7ca0bb8036d2eeb1a0e1dcf624fcab14d99d0f692a49c73b4138cb385609af
    while let Some(position) = line[from..].find(MARKER) {
        let at = from + position;
        // The comment opener of this marker: the last one before it, which the marker is
        // written after as the guidance says.
        if let Some(opener) = line[..at].rfind(comment) {
            return line[..opener].trim_end();
        }
        from = at + MARKER.len();
    }
    line
}

/// What every marker begins with, after the target's comment opener.
// @lfy def/cli/main.lfy:main#main:main:3c7ca0bb8036d2eeb1a0e1dcf624fcab14d99d0f692a49c73b4138cb385609af
pub(crate) const MARKER: &str = "@lfy ";

/// One region named the way a marker spells it: the file, the entity or the line, and the
/// requirement when it answers for one. Two markers of one region spell the same name.
// @lfy def/cli/main.lfy:main#main:main:94482ac68d310e72da7940331629312470c58e04dbf09548f66eb20565752b91
pub(crate) fn region_named(marker: &Marker) -> String {
    let place = match &marker.entity {
        Some(entity) => entity.clone(),
        None => marker.line.to_string(),
    };
    // @lfy def/cli/main.lfy:main#main:main:94482ac68d310e72da7940331629312470c58e04dbf09548f66eb20565752b91
    match &marker.requirement {
        Some(requirement) => format!("{}:{place}#{requirement}", marker.file),
        None => format!("{}:{place}", marker.file),
    }
}

/// What the last review of one unit found: the reviews of its last run of the verifier, and
/// the hashes and carried verdicts of the review request that run was made with. The review
/// records of the unit are written from these when its source maps are recorded.
// @lfy def/cli/main.lfy:main#main:main:05ecd897c6044e88ca2bad408d90091093bb8d358a4cb29d412daaab6695e86c
#[derive(Clone, Default)]
pub(crate) struct Reviewed {
    /// The reviews of the unit's last verifier run.
    pub(crate) reviews: Vec<Review>,
    /// The hash that run's review request gave each id.
    pub(crate) hashes: BTreeMap<String, String>,
    /// The verdicts that review request carried.
    pub(crate) carried: Vec<ReviewRecord>,
}

/// How a unit that stopped without being accepted stopped.
// @lfy def/cli/main.lfy:main#main:main:6e192fbbaa3d541cc1fa9490c15245bc628f83977d91727e7004d801f77b9104
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    /// Rejected after its retries.
    // @lfy def/cli/main.lfy:main#main:main:0954f463a673ef9da817b979310906a5a5009931bd64cc4941c7822e82a8d017
    Rejected,
    /// The compiler could not proceed.
    // @lfy def/cli/main.lfy:main#main:main:758fa1871b09f337b51e44d8a8f6104962caf305712f1929d105dc578aa66787
    Blocked,
    /// The compiler asked a question.
    // @lfy def/cli/main.lfy:main#main:main:c23e2d99fb15c650e2042ab1842fb2dbcc75b210acde889bbdc12eb9f947e652
    Question,
    /// The compiler command failed a second time.
    // @lfy def/cli/main.lfy:main#main:main:3ac90f9999308a6d63417d712d756867c10bc89fc52537afa640a0e82a6f5ba2
    Failed,
    /// Its merge was undone twice in a row.
    // @lfy def/cli/main.lfy:main#main:main:ee6f110a6e8a80fe7f058125db6f1cd190ee42098edb6f0deaed19ccf0a70a19
    Undone,
}

/// What one compile is doing: the plan and the inputs it was made from, the source maps as
/// they are recorded in the root, the change set of each target it works, what it counts for
/// the finished line, and the units that stopped or were skipped.
// @lfy def/cli/main.lfy:main
pub(crate) struct Run {
    /// The workspace lowered: what the plan, the requests, and the reviews are read from.
    // @lfy def/cli/main.lfy:main
    pub(crate) program: Program,
    pub(crate) plan: Plan,
    /// What the compile's first plan was made from, so that the plan can be made again with
    /// the foreign edits recorded since in place of those it was given.
    // @lfy def/cli/main.lfy:main#main:main:fc8d9730ea95e71cb418a2d3c7249b91a9f7ca20d5041d67b3296632614122c8
    pub(crate) planning: Planning,
    /// The source maps recorded in the root when the compile began.
    pub(crate) maps: Vec<SourceMap>,
    pub(crate) reporter: Reporter,
    /// What running the compiler and the verifier takes.
    // @lfy def/cli/main.lfy:main#main:main:3794ecd87cdab206cb18cf520d802e324546a2124e9c3ab4f934dc88f93dbb58
    pub(crate) agent: Agent,
    /// The process groups running, so that a compile that stops stops them.
    // @lfy def/cli/main.lfy:main#main:main:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
    pub(crate) running: Arc<Running>,
    pub(crate) json: bool,
    /// Whether each stream is painted.
    pub(crate) style: Paint,
    pub(crate) continuing: bool,
    /// The command elfie.json names under `compiler`.
    pub(crate) compiler: Option<String>,
    /// The command elfie.json names under `verifier`; nothing verifies a unit without one.
    // @lfy def/cli/main.lfy:main#main:main:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
    pub(crate) verifier: Option<String>,
    /// --no-verify: no verifier runs and nothing is reviewed.
    // @lfy def/cli/main.lfy:main#main:main:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
    pub(crate) no_verify: bool,
    /// --accept: no compiler is run, and no review record file changes.
    // @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
    pub(crate) accept_only: bool,
    /// --full: every criterion and test of a unit is reviewed again, carrying nothing.
    // @lfy def/cli/main.lfy:main#main:main:8b2d4cb145c85977997b07a3824a4854f7eecccbcae9af2711ec792f26fdb7a9
    pub(crate) full: bool,
    /// The target --target names; every target when it is not given.
    pub(crate) target: Option<String>,
    /// The change set of each target the compile works, by its identifier.
    // @lfy def/cli/main.lfy:main#main:main:52dc0204d72665f32b60eef1095ce6a73d89469226840fd8606ff7d622cc4745
    pub(crate) sets: BTreeMap<String, ChangeSet>,
    /// Where each planned unit that no change set holds stands, by its place in the plan.
    pub(crate) states: BTreeMap<usize, UnitState>,
    /// What the last review of each unit found, by its place in the plan.
    // @lfy def/cli/main.lfy:main#main:main:05ecd897c6044e88ca2bad408d90091093bb8d358a4cb29d412daaab6695e86c
    pub(crate) reviewed: BTreeMap<usize, Reviewed>,
    /// The units that stopped without being accepted, by their place in the plan.
    // @lfy def/cli/main.lfy:main#main:main:983a29cf93655951616c5c562ff1c6178966fa0f7acb433740f541446615af08
    pub(crate) stopped: BTreeSet<usize>,
    /// The units skipped because a unit they depend on stopped.
    // @lfy def/cli/main.lfy:main#main:main:983a29cf93655951616c5c562ff1c6178966fa0f7acb433740f541446615af08
    pub(crate) skipped: BTreeSet<usize>,
    /// Whether the compile stopped: no unit starts after that.
    // @lfy def/cli/main.lfy:main#main:main:51b22c76c1410a187c0f287178a9ecb28048b54a983754ad873fa703eb7809a3
    pub(crate) halted: bool,
    /// Whether a final gate, the global review, or the merge failed.
    // @lfy def/cli/main.lfy:main#main:main:3bd28015ddcd6d08a3df7365b38b07af984f8563916c03a19a03c140696c78ce
    pub(crate) gate_failed: bool,
    pub(crate) rejected: usize,
    pub(crate) blocked: usize,
    pub(crate) failed: usize,
    pub(crate) code: ExitCode,
    /// How many units may run at once, across every target: what --jobs gives, or 4.
    // @lfy def/cli/main.lfy:main#main:main:1fd9be2beb849bc4a65969dbedc075ab348b763ef108d6fbc41f4001bc8a86b9
    pub(crate) jobs: usize,
    /// The compile's relocation, or verify's, in scope of --target: its relocations, which
    /// every request is given, and the review records it moved, by unit.
    // @lfy def/cli/main.lfy:main#main:main:2b2ecf83e7e057bba8be8cefb6ab9f0de30c74dfba2269df5b9fb4915457dec3
    // @lfy def/cli/main.lfy:main#main:main:e51fb9eb0e95db88123897a941b44ce09dcb60390faac4e1c4cc0afab6a7c22b
    pub(crate) relocation: Relocated,
    /// The target and source file of each unit every map of whose outputs the compile's
    /// relocation made, the unit having no map of its own before it.
    // @lfy def/cli/main.lfy:main#main:main:56dfba76b54a5c6836f292e001f259f857a7245ccec2e5b571ddfb7f62347513
    pub(crate) relocated_only: BTreeSet<(String, String)>,
    /// The previous source of each unit, by its place in the plan, recovered once in the
    /// compile and the same however many times the unit runs; `None` for a unit that has none.
    // @lfy def/cli/main.lfy:main#main:main:021113949b86da06e8b9d60a92d6451edc22619f4e586d3549313c4be3c71d4f
    pub(crate) previous: BTreeMap<usize, Option<Previous>>,
    /// The foreign edits a unit's run in its copy is given, by its place in the plan: those a
    /// unit its copy holds the merge of made.
    // @lfy def/cli/main.lfy:main#main:main:5b241bf72a28eb83044e40272acd76f441efff41a88bd21769255cca804fdc4a
    pub(crate) edits: BTreeMap<usize, Vec<ForeignEdit>>,
    /// Where the units of each change set stand while they run at once.
    // @lfy def/cli/main.lfy:main#main:main:8cf4943ef6637dd81797621b8abc048edcc7781a4f19546fbe99863339043826
    pub(crate) schedule: crate::schedule::Schedule,
    /// Whether this run works one unit in its copy, apart from the compile that started it,
    /// so that nothing of a change set is touched by it.
    // @lfy def/cli/main.lfy:main#main:main:649dfaca80ede5a5de3cfecd48f1de664e250e9bc65773266822f3f63ae34c71
    pub(crate) worker: bool,
    /// The program with a directory other than the root as its workspace's root, made once
    /// for that directory, so that what is recorded for a unit worked in a copy is written
    /// there.
    // @lfy def/cli/main.lfy:main#main:main:5a9f0a1a607bedde3d9614508a9e4477732de6ab40aa00c924024f8a13bded91
    moved: Option<(PathBuf, Program)>,
}

impl Run {
    // @lfy def/cli/main.lfy:main
    pub(crate) fn new(
        program: Program,
        plan: Plan,
        planning: Planning,
        reporter: Reporter,
        invocation: &Invocation,
    ) -> Run {
        let root = program.workspace.root.clone();
        // @lfy def/cli/main.lfy:main#main:main:3794ecd87cdab206cb18cf520d802e324546a2124e9c3ab4f934dc88f93dbb58
        let agent = Agent::new(invocation.flag("json"), Paint::of(invocation), reporter.logs.clone(), &root);
        Run {
            maps: planning.maps.clone(),
            program,
            plan,
            planning,
            reporter,
            agent,
            running: Arc::new(Running::default()),
            json: invocation.flag("json"),
            style: Paint::of(invocation),
            continuing: invocation.flag("continue"),
            compiler: compiler_command(&root),
            verifier: verifier_command(&root),
            no_verify: invocation.flag("no-verify"),
            accept_only: invocation.flag("accept"),
            full: invocation.flag("full"),
            target: invocation.option("target").map(str::to_string),
            sets: BTreeMap::new(),
            states: BTreeMap::new(),
            reviewed: BTreeMap::new(),
            stopped: BTreeSet::new(),
            skipped: BTreeSet::new(),
            halted: false,
            gate_failed: false,
            rejected: 0,
            blocked: 0,
            failed: 0,
            code: ExitCode::Success,
            // @lfy def/cli/main.lfy:main#main:main:1fd9be2beb849bc4a65969dbedc075ab348b763ef108d6fbc41f4001bc8a86b9
            jobs: invocation.option(crate::commands::JOBS).and_then(crate::commands::jobs_of).unwrap_or(DEFAULT_JOBS),
            relocation: Relocated::default(),
            relocated_only: BTreeSet::new(),
            previous: BTreeMap::new(),
            edits: BTreeMap::new(),
            schedule: crate::schedule::Schedule::default(),
            worker: false,
            moved: None,
        }
    }

    /// A run of its own for one unit worked in its copy, apart from this compile: the same
    /// plan, program, and options, printing through the same reporter and stopped with the
    /// same process groups, given the unit's previous source and the foreign edits its run is
    /// given, and holding no change set, so that nothing it does reaches one but through what
    /// it gives back.
    // @lfy def/cli/main.lfy:main#main:main:649dfaca80ede5a5de3cfecd48f1de664e250e9bc65773266822f3f63ae34c71
    // @lfy def/cli/main.lfy:main#main:main:91514d592807a6eae69b04199835ae037c731478013def307e41bb393d1f98c3
    pub(crate) fn fork(&self, unit: usize, edits: Vec<ForeignEdit>) -> Run {
        let mut previous = BTreeMap::new();
        if let Some(found) = self.previous.get(&unit) {
            previous.insert(unit, found.clone());
        }
        Run {
            program: self.program.clone(),
            plan: self.plan.clone(),
            planning: self.planning.clone(),
            maps: self.maps.clone(),
            reporter: self.reporter.clone(),
            agent: self.agent.clone(),
            running: self.running.clone(),
            json: self.json,
            style: self.style,
            continuing: self.continuing,
            compiler: self.compiler.clone(),
            verifier: self.verifier.clone(),
            no_verify: self.no_verify,
            accept_only: self.accept_only,
            full: self.full,
            target: self.target.clone(),
            sets: BTreeMap::new(),
            states: BTreeMap::new(),
            reviewed: BTreeMap::new(),
            stopped: BTreeSet::new(),
            skipped: BTreeSet::new(),
            halted: false,
            gate_failed: false,
            rejected: 0,
            blocked: 0,
            failed: 0,
            code: ExitCode::Success,
            jobs: self.jobs,
            relocation: self.relocation.clone(),
            relocated_only: self.relocated_only.clone(),
            previous,
            edits: [(unit, edits)].into(),
            schedule: crate::schedule::Schedule::default(),
            worker: true,
            moved: None,
        }
    }

    /// A unit's previous source: `previousOf` of the program the plan was made from, the
    /// unit, and its outputs, recovered once in the compile and kept, so that the rewording,
    /// every request, `reworded`, and the checks of the steps without the compiler are given
    /// the same one. A unit with no outputs, or every map of whose outputs the compile's
    /// relocation made for a unit with no map of its own, has none, and nothing is recovered
    /// for it.
    // @lfy def/cli/main.lfy:main#main:main:021113949b86da06e8b9d60a92d6451edc22619f4e586d3549313c4be3c71d4f
    // @lfy def/cli/main.lfy:main#main:main:56dfba76b54a5c6836f292e001f259f857a7245ccec2e5b571ddfb7f62347513
    pub(crate) fn recover(&mut self, unit: usize) {
        if self.previous.contains_key(&unit) {
            return;
        }
        let found = self.recovered(unit);
        self.previous.insert(unit, found);
    }

    /// The previous source of a unit, as [`Run::recover`] keeps it, or recovered now when it
    /// was never kept.
    // @lfy def/cli/main.lfy:main#main:main:021113949b86da06e8b9d60a92d6451edc22619f4e586d3549313c4be3c71d4f
    pub(crate) fn previous_source(&self, unit: usize) -> Option<Previous> {
        match self.previous.get(&unit) {
            Some(found) => found.clone(),
            None => self.recovered(unit),
        }
    }

    /// `previousOf` called for a unit, or nothing for a unit that has no previous source.
    // @lfy def/cli/main.lfy:main#main:main:021113949b86da06e8b9d60a92d6451edc22619f4e586d3549313c4be3c71d4f
    // @lfy def/cli/main.lfy:main#main:main:56dfba76b54a5c6836f292e001f259f857a7245ccec2e5b571ddfb7f62347513
    fn recovered(&self, unit: usize) -> Option<Previous> {
        let planned = &self.plan.units[unit];
        // @lfy def/cli/main.lfy:main#main:main:56dfba76b54a5c6836f292e001f259f857a7245ccec2e5b571ddfb7f62347513
        if planned.outputs.is_empty() {
            return None;
        }
        let key = (self.target_of_unit(unit).identifier.clone(), self.workspace().files[planned.file].path.clone());
        if self.relocated_only.contains(&key) {
            return None;
        }
        // @lfy def/cli/main.lfy:main#main:main:021113949b86da06e8b9d60a92d6451edc22619f4e586d3549313c4be3c71d4f
        generation::previous::previous_of(&self.program, planned, &planned.outputs)
    }

    /// The workspace the plan was made from.
    pub(crate) fn workspace(&self) -> &Workspace {
        &self.program.workspace
    }

    /// The root of the workspace.
    pub(crate) fn root(&self) -> PathBuf {
        self.program.workspace.root.clone()
    }

    /// The target of one unit of the plan.
    pub(crate) fn target_of_unit(&self, unit: usize) -> &Target {
        &self.program.workspace.targets[self.plan.units[unit].target]
    }

    /// The stem of one unit of the plan.
    pub(crate) fn stem(&self, unit: usize) -> String {
        self.plan.units[unit].stem.clone()
    }

    /// Whether a verifier is to review what is accepted.
    // @lfy def/cli/main.lfy:main#main:main:da396078b042c11950a5c194c204293f329886e37ab0ea41c0a36030c64d3d56
    pub(crate) fn verifies(&self) -> bool {
        self.verifier.is_some() && !self.no_verify
    }

    /// The code so far, or a new one when it is more severe.
    // @lfy def/cli/main.lfy:main
    pub(crate) fn worsen(&mut self, code: ExitCode) {
        if code.rank() > self.code.rank() {
            self.code = code;
        }
    }

    /// The plan and the program replaced by ones made again; what the program was relocated
    /// to is made again with it.
    // @lfy def/cli/main.lfy:main#main:main:fc8d9730ea95e71cb418a2d3c7249b91a9f7ca20d5041d67b3296632614122c8
    pub(crate) fn replan(&mut self, program: Program, plan: Plan) {
        self.program = program;
        self.plan = plan;
        self.moved = None;
    }

    /// The program with a directory as its workspace's root: the program itself for the
    /// root, and for a copy the same program with the copy as its root, so that `record`,
    /// `rederive`, `recordReviews`, `reviewsOf`, and `relocate` read and write there and never
    /// in the root.
    // @lfy def/cli/main.lfy:main#main:main:5a9f0a1a607bedde3d9614508a9e4477732de6ab40aa00c924024f8a13bded91
    pub(crate) fn program_at(&mut self, directory: &Path) -> &Program {
        if directory == self.program.workspace.root {
            return &self.program;
        }
        if self.moved.as_ref().is_none_or(|(at, _)| at != directory) {
            let mut moved = self.program.clone();
            // @lfy def/cli/main.lfy:main#main:main:5a9f0a1a607bedde3d9614508a9e4477732de6ab40aa00c924024f8a13bded91
            moved.workspace.root = directory.to_path_buf();
            self.moved = Some((directory.to_path_buf(), moved));
        }
        &self.moved.as_ref().expect("moved just now").1
    }

    /// The change set a unit is worked in, when the compile works one for its target.
    // @lfy def/cli/main.lfy:main#main:main:52dc0204d72665f32b60eef1095ce6a73d89469226840fd8606ff7d622cc4745
    pub(crate) fn set_of(&self, unit: usize) -> Option<&ChangeSet> {
        self.sets.get(&self.target_of_unit(unit).identifier)
    }

    /// A unit's target's working directory: its change set's copy when the compile works one
    /// for its target, and the root otherwise.
    // @lfy def/cli/main.lfy:main#main:main:966ee6d2b294a438d616849466eda454045e72dc5098c02c6025b33a7e34c829
    pub(crate) fn working_directory(&self, unit: usize) -> PathBuf {
        match self.set_of(unit) {
            Some(set) => PathBuf::from(&set.directory),
            None => self.root(),
        }
    }

    /// Where a planned unit stands: its state in its change set, or as the compile counted it
    /// when no change set holds it; planned when nothing says otherwise.
    // @lfy def/cli/main.lfy:main#main:main:4fb67ab23f1c2255a1177088bce0458b4c7fc72eee0b9f60cc2c47cdc0e85480
    pub(crate) fn state_of(&self, unit: usize) -> UnitState {
        let stem = &self.plan.units[unit].stem;
        match self.set_of(unit) {
            Some(set) => set.units.iter().find(|held| &held.stem == stem).map_or(UnitState::Planned, |held| held.state),
            None => self.states.get(&unit).copied().unwrap_or(UnitState::Planned),
        }
    }

    /// A unit's state set, in its change set or as the compile counts it; the change set's
    /// file is written at once, and the count of units done follows.
    // @lfy def/cli/main.lfy:main#main:main:3d654e560f2e1144270b416e60c71b73f41429d32f691bba2f8f6fab0038bd7a
    pub(crate) fn set_state(&mut self, unit: usize, state: UnitState) {
        let stem = self.stem(unit);
        let target = self.target_of_unit(unit).identifier.clone();
        match self.sets.get_mut(&target) {
            Some(set) => {
                if let Some(held) = set.units.iter_mut().find(|held| held.stem == stem) {
                    held.state = state;
                }
                self.save(&target);
            }
            None => {
                self.states.insert(unit, state);
            }
        }
        self.count_done();
    }

    /// One change set written to its file under elfie-compile/cache, at once.
    // @lfy def/cli/main.lfy:main#main:main:3d654e560f2e1144270b416e60c71b73f41429d32f691bba2f8f6fab0038bd7a
    pub(crate) fn save(&mut self, target: &str) {
        let root = self.root();
        let Some(set) = self.sets.get(target) else { return };
        // @lfy def/cli/main.lfy:main#main:main:3d654e560f2e1144270b416e60c71b73f41429d32f691bba2f8f6fab0038bd7a
        if let Err(error) = set.save(&root) {
            let path = ChangeSet::file(&root, target).display().to_string();
            complain(self.style.err, Some(&path), &error.to_string());
            self.worsen(ExitCode::Failure);
        }
    }

    /// The planned units of the compile, in plan order: every unit of a change set, and every
    /// unit with a reason of the targets --target leaves in.
    pub(crate) fn planned_units(&self) -> Vec<usize> {
        (0..self.plan.units.len())
            .filter(|&index| {
                let unit = &self.plan.units[index];
                let target = target_of(&self.program.workspace, unit);
                if self.target.as_deref().is_some_and(|named| named != target) {
                    return false;
                }
                match self.sets.get(target) {
                    Some(set) => set.units.iter().any(|held| held.stem == unit.stem),
                    None => unit.reason.is_some(),
                }
            })
            .collect()
    }

    /// The count of units accepted so far, as every progress line shows it, and of units
    /// planned.
    // @lfy def/cli/main.lfy:main#main:main:4fb67ab23f1c2255a1177088bce0458b4c7fc72eee0b9f60cc2c47cdc0e85480
    pub(crate) fn count_done(&mut self) {
        // A unit worked in its copy holds no change set, so what it would count is not what
        // the compile counts.
        if self.worker {
            return;
        }
        let planned = self.planned_units();
        let done = planned.iter().filter(|&&unit| self.state_of(unit).is_done()).count();
        self.reporter.set_counts(done, planned.len());
    }

    /// The directory a unit's requests, questions, and reasons are written to:
    /// `elfie-requests` under the root, never under an output directory.
    // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
    pub(crate) fn requests_directory(&self) -> PathBuf {
        requests_directory(&self.workspace().root)
    }

    /// Writes `elfie-requests/<name>.<suffix>` under the root, with `<name>` the request name
    /// given.
    // @lfy def/cli/main.lfy:main#main:main:471b5ff5941e414fe0f5f5350c3d0f8c3560fdbfd6326589991ff388f7367356
    pub(crate) fn write_note(&self, name: &str, suffix: &str, text: &str) -> Option<PathBuf> {
        let directory = self.requests_directory();
        let path = directory.join(format!("{}.{suffix}", request_name(name)));
        // The folder it writes into is created when it is missing.
        // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
        match fs::create_dir_all(&directory).and_then(|()| fs::write(&path, text)) {
            Ok(()) => Some(path),
            Err(error) => {
                // Any other message the CLI itself prints to standard error begins with
                // elfie: painted failure, and a message that begins with a path has the
                // path painted subject.
                // @lfy def/cli/main.lfy:main#main:main:e08f0e5222d67e3b1a8262febafcc033512185f2bbde7fbe34f189a53fb1d3f1
                complain(self.style.err, Some(&path.to_string_lossy()), &error.to_string());
                None
            }
        }
    }

    /// The answer a person wrote below the question in `elfie-requests/<name>.question.md`,
    /// when there is one.
    // @lfy def/cli/main.lfy:main#main:main:1589ce12c96cdf908edd65828b29cd2a2ced6c580a9f214a9cfb2030bf2539ce
    pub(crate) fn answer_of(&self, stem: &str) -> Option<String> {
        let path = self.requests_directory().join(format!("{}.question.md", request_name(stem)));
        let text = fs::read_to_string(path).ok()?;
        let (_, below) = text.split_once(ANSWER_HEADING)?;
        let answer = below
            .lines()
            .filter(|line| !line.trim_start().starts_with("<!--"))
            .collect::<Vec<_>>()
            .join("\n");
        let answer = answer.trim().to_string();
        (!answer.is_empty()).then_some(answer)
    }

    /// The request of one unit, with its existing outputs read from its directory, its
    /// previous source, the foreign edits given, the relocations of the compile, and the
    /// answer to a question asked before appended.
    ///
    /// `hosted` is whether the request is for a compiler run in a hosted session of this
    /// command's agent server: true for the request handed to a compiler command, false for
    /// the one written out for a compiler run by hand. The second value is whether the unit's
    /// previous source is defined and is not only its hash-matched source.
    // @lfy def/cli/main.lfy:main#main:main:3794ecd87cdab206cb18cf520d802e324546a2124e9c3ab4f934dc88f93dbb58
    // @lfy def/cli/main.lfy:main#main:main:c3dec788ff5d654d7dbac2d0021b2e65e093abd3a174484ae34c58ff953c779a
    // @lfy def/cli/main.lfy:main#main:main:2b2ecf83e7e057bba8be8cefb6ab9f0de30c74dfba2269df5b9fb4915457dec3
    pub(crate) fn request_of(
        &self,
        unit: usize,
        directory: &Path,
        hosted: bool,
        foreign_edits: &[ForeignEdit],
    ) -> (Request, bool) {
        // The files of the output directory whose text carries a marker for the unit.
        // @lfy def/cli/main.lfy:main#main:main:c3dec788ff5d654d7dbac2d0021b2e65e093abd3a174484ae34c58ff953c779a
        let (existing, _) = outputs_of(self.workspace(), &self.plan, unit, directory);
        // The unit's previous source as previous, the same every time the unit is requested,
        // and the relocations of the compile as relocations.
        // @lfy def/cli/main.lfy:main#main:main:2b2ecf83e7e057bba8be8cefb6ab9f0de30c74dfba2269df5b9fb4915457dec3
        // @lfy def/cli/main.lfy:main#main:main:021113949b86da06e8b9d60a92d6451edc22619f4e586d3549313c4be3c71d4f
        let was = self.previous_source(unit);
        let accepted_source = was.as_ref().is_some_and(|found| !found.hash_matched);
        let mut request = generation::request(
            &self.program,
            &self.plan,
            unit,
            &existing,
            was.as_ref(),
            hosted,
            foreign_edits,
            &self.relocation.relocations,
        );
        // The person answers by editing the definitions, or by writing the answer below
        // the question, which the next compile of the unit appends to its instructions.
        // @lfy def/cli/main.lfy:main#main:main:1589ce12c96cdf908edd65828b29cd2a2ced6c580a9f214a9cfb2030bf2539ce
        if let Some(answer) = self.answer_of(&self.plan.units[unit].stem) {
            request.instructions.push_str(&format!("\n\n## The answer to the question asked before\n\n{answer}\n"));
        }
        (request, accepted_source)
    }

    /// Whether every output of a unit has the unit's own region text that [`SourceMap::text`]
    /// of its map holds: the outputs on disk are exactly the ones the last acceptance
    /// recorded, so nothing of them has to be generated again.
    ///
    /// Both sides are read in the unit's directory: the output, and the unit's maps from its
    /// map file there, so maps re-derived before the comparison count. The own region text is
    /// hashed as SHA-256 in lowercase hex. A unit with no map there has nothing on disk that
    /// stands, so it never holds, and neither does a map recorded before the text hash was,
    /// whose empty hash no text equals.
    // @lfy def/cli/main.lfy:main#main:main:3c7ca0bb8036d2eeb1a0e1dcf624fcab14d99d0f692a49c73b4138cb385609af
    // @lfy def/cli/main.lfy:main#main:main:92dda467736030409d0aa3d11b09f0e0dfe83df48a8098c9b062fb817c2ed45e
    pub(crate) fn outputs_stand(&self, unit: usize, directory: &Path) -> bool {
        // @lfy def/cli/main.lfy:main#main:main:3c7ca0bb8036d2eeb1a0e1dcf624fcab14d99d0f692a49c73b4138cb385609af
        let maps = self.maps_in(unit, directory);
        !maps.is_empty()
            && maps.iter().all(|map| {
                fs::read_to_string(directory.join(&map.output)).is_ok_and(|text| {
                    // @lfy def/cli/main.lfy:main#main:main:3c7ca0bb8036d2eeb1a0e1dcf624fcab14d99d0f692a49c73b4138cb385609af
                    let own = own_region_text(self.workspace(), &self.plan, unit, &text);
                    generation::source_hash(&own) == map.text
                })
            })
    }

    /// The source maps of one unit as the map file in a directory holds them, as recorded:
    /// whether or not the outputs they name still exist, which is how `sourceMapsOf` reads a
    /// map file apart from that.
    // @lfy def/cli/main.lfy:main#main:main:8940bcdd4260c6e311e87a27ce3497056d6d8956860d16464a361158ed9f48da
    pub(crate) fn maps_in(&self, unit: usize, directory: &Path) -> Vec<SourceMap> {
        generation::read_source_maps(&self.map_file_in(unit, directory))
    }

    /// The map file of one unit under a directory, which is the root or a copy of it, so a
    /// unit's maps are named the same either way.
    // @lfy def/cli/main.lfy:main#main:main:8940bcdd4260c6e311e87a27ce3497056d6d8956860d16464a361158ed9f48da
    pub(crate) fn map_file_in(&self, unit: usize, directory: &Path) -> PathBuf {
        let unit = &self.plan.units[unit];
        directory
            .join(MAPS)
            .join(&self.workspace().targets[unit.target].identifier)
            .join(format!("{}.json", unit.stem))
    }

    /// The markers of a unit's reworded criteria and tests rewritten to the ids they have now,
    /// in the outputs in the unit's directory, before a request is made for the unit.
    ///
    /// Nothing is rewritten for a unit with no previous source, or whose previous source is
    /// only its hash-matched source, since nothing then says which id replaces which. One
    /// rewording progress line counts its reworded criteria and tests and the outputs written.
    // @lfy def/cli/main.lfy:main#main:main:5addc4adee4b879e13c461f3ad0fe117798288430fd32f3f1e0360099beffc41
    // @lfy def/cli/main.lfy:main#main:main:5731809a4610bdcc66b727d15eee1aa0e72029842df8f2e0414f6d054e2ea8c0
    // @lfy def/cli/main.lfy:main#main:main:fa18c20576f142bd6c6ed82dce02b8ba5a122973c56fb0aae68b0c40a74c201c
    pub(crate) fn reword_outputs(&mut self, unit: usize, directory: &Path) {
        let planned = self.plan.units[unit].clone();
        // Only a previous source that is not only the hash-matched one says which id
        // replaces which; the hash-matched source answered for other ids altogether.
        // @lfy def/cli/main.lfy:main#main:main:5addc4adee4b879e13c461f3ad0fe117798288430fd32f3f1e0360099beffc41
        let Some(was) = self.previous_source(unit).filter(|found| !found.hash_matched) else {
            return;
        };
        // @lfy def/cli/main.lfy:main#main:main:5addc4adee4b879e13c461f3ad0fe117798288430fd32f3f1e0360099beffc41
        let rewordings = generation::reworded(&self.program, &planned, &was);
        if rewordings.is_empty() {
            return;
        }
        // The id each marker names now, by the id it names on disk.
        // @lfy def/cli/main.lfy:main#main:main:5addc4adee4b879e13c461f3ad0fe117798288430fd32f3f1e0360099beffc41
        let ids: BTreeMap<String, String> =
            rewordings.iter().map(|rewording| (rewording.replaces.clone(), rewording.id.clone())).collect();
        let (outputs, _) = outputs_of(self.workspace(), &self.plan, unit, directory);
        // @lfy def/cli/main.lfy:main#main:main:5addc4adee4b879e13c461f3ad0fe117798288430fd32f3f1e0360099beffc41
        let renamed = generation::rename(self.workspace(), &self.plan, unit, &outputs, &ids);
        let mut written = 0;
        for output in &renamed {
            let path = directory.join(&output.path);
            // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            // @lfy def/cli/main.lfy:main#main:main:5addc4adee4b879e13c461f3ad0fe117798288430fd32f3f1e0360099beffc41
            match fs::write(&path, &output.text) {
                Ok(()) => written += 1,
                Err(error) => {
                    complain(self.style.err, Some(&output.path), &error.to_string());
                    self.worsen(ExitCode::Failure);
                }
            }
        }
        // @lfy def/cli/main.lfy:main#main:main:fa18c20576f142bd6c6ed82dce02b8ba5a122973c56fb0aae68b0c40a74c201c
        if written > 0 {
            let message = format!("{} reworded, {written} outputs written", rewordings.len());
            self.reporter.report(Step::Rewording, Some(&planned.stem), &message);
        }
    }

    /// The verdict of `accept` on a unit's outputs in its directory: the files under the
    /// target's output directory there that carry a marker for the unit, with the source maps
    /// recorded for the target's other units as the map files there hold them, every other
    /// file of that listing as elsewhere, the files given as the ones before the unit's first
    /// run, and the notes given for the units it touched.
    // @lfy def/cli/main.lfy:main#main:main:91ffd2f6eafdd00f74f9012574d3c7ae2bd6569a8b89e66b9bfb6b117a6c8488
    // @lfy def/cli/main.lfy:main#main:main:469c6230ef224ea4f0cd3756084725704c81af1b734d9ca5e217dc12f854edb8
    pub(crate) fn verdict_of(
        &mut self,
        unit: usize,
        request: &Request,
        directory: &Path,
        before: Option<&[Output]>,
        touches: &BTreeMap<String, String>,
    ) -> Verdict {
        let stem = self.stem(unit);
        let reason = self.plan.units[unit].reason.map_or_else(|| "up to date".to_string(), |r| r.as_str().to_string());
        self.reporter.report(Step::Checking, Some(&stem), &reason);
        // @lfy def/cli/main.lfy:main#main:main:91ffd2f6eafdd00f74f9012574d3c7ae2bd6569a8b89e66b9bfb6b117a6c8488
        let (outputs, elsewhere) = outputs_of(self.workspace(), &self.plan, unit, directory);
        // The source maps recorded for the target's other units, which say which outputs
        // already belong to another unit.
        // @lfy def/cli/main.lfy:main#main:main:91ffd2f6eafdd00f74f9012574d3c7ae2bd6569a8b89e66b9bfb6b117a6c8488
        let recorded = self.recorded_elsewhere(unit, directory);
        generation::accept(
            &self.program,
            &self.plan,
            request,
            unit,
            &outputs,
            &recorded,
            &elsewhere,
            before,
            touches,
        )
    }

    /// The source maps recorded for the other units of one unit's target, read from the map
    /// files in the unit's directory, as recorded: what `accept` reads to see whether an
    /// output already belongs to another unit, and where another unit's regions are.
    // @lfy def/cli/main.lfy:main#main:main:91ffd2f6eafdd00f74f9012574d3c7ae2bd6569a8b89e66b9bfb6b117a6c8488
    // @lfy def/cli/main.lfy:main#main:main:6b6776d9051184c3c3b68bf73d7695e20b6854877e1970d17e5ef917f80dd7c0
    pub(crate) fn recorded_elsewhere(&self, unit: usize, directory: &Path) -> Vec<SourceMap> {
        let target = self.plan.units[unit].target;
        let source = self.plan.units[unit].file;
        let mut out: Vec<SourceMap> = Vec::new();
        for other in 0..self.plan.units.len() {
            let held = &self.plan.units[other];
            if held.target != target || held.file == source {
                continue;
            }
            out.extend(self.maps_in(other, directory));
        }
        out
    }

    /// A unit's outputs written back in its directory, with their markers normalized.
    // @lfy def/cli/main.lfy:main#main:main:87ae3376d10498cdfb807849c1ba3d8243944e7c065752aafa1a959603280213
    pub(crate) fn write_back(&mut self, unit: usize, verdict: &Verdict, directory: &Path) -> bool {
        for output in &verdict.outputs {
            let path = directory.join(&output.path);
            // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
            let written = path
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| fs::write(&path, &output.text));
            // @lfy def/cli/main.lfy:main#main:main:87ae3376d10498cdfb807849c1ba3d8243944e7c065752aafa1a959603280213
            if let Err(error) = written {
                let stem = self.stem(unit);
                complain(self.style.err, Some(&stem), &error.to_string());
                self.worsen(ExitCode::Failure);
                return false;
            }
        }
        true
    }

    /// The source maps of a unit recorded in its directory by `record`, the review records of
    /// a unit that was verified written beside them by `recordReviews`, and every co-owner's
    /// maps re-derived there once they are.
    ///
    /// Recording one unit leaves every other map file byte for byte as it was, a co-owner's
    /// aside.
    // @lfy def/cli/main.lfy:main#main:main:581f8ca189aaf93dbb2a251ac39e808b7454c0cc223e94b9a5369e0dafdff965
    // @lfy def/cli/main.lfy:main#main:main:1a380544042dc2a265daa89f48fba57d42b2227401fa8e393f68af08ae06fc21
    // @lfy def/cli/main.lfy:main#main:main:73b5383aed1ca86a8561196285ac24e93f2aa3b1cf5665ccb3c44c99c8bcfb1f
    // @lfy def/cli/main.lfy:main#main:main:903564ed1cc08c957bb20abfdd0ab90ad696be6427d003cd21a97daf964f1b09
    pub(crate) fn record_maps(&mut self, unit: usize, maps: &[SourceMap], directory: &Path, reviewed: bool) {
        // The files the unit's maps named before they were recorded, read off its map file
        // in its directory: a unit whose map file names one of them is a co-owner too, since
        // this unit has just stopped writing a file it used to share with it.
        // @lfy def/cli/main.lfy:main#main:main:73b5383aed1ca86a8561196285ac24e93f2aa3b1cf5665ccb3c44c99c8bcfb1f
        let mut shared: BTreeSet<String> = self.maps_in(unit, directory).into_iter().map(|map| map.output).collect();
        let recorded: BTreeSet<String> = maps.iter().map(|map| map.output.clone()).collect();
        shared.extend(recorded.iter().cloned());
        let planned = self.plan.units[unit].clone();
        let program = self.program_at(directory);
        // @lfy def/cli/main.lfy:main#main:main:5a9f0a1a607bedde3d9614508a9e4477732de6ab40aa00c924024f8a13bded91
        let written = generation::record(&program.workspace, &planned, maps);
        let path = generation::map_file(&program.workspace, &planned).display().to_string();
        if !written {
            complain(self.style.err, Some(&path), "could not be written");
            self.worsen(ExitCode::Failure);
        }
        // The review records of a unit that was verified, written in its directory.
        // @lfy def/cli/main.lfy:main#main:main:05ecd897c6044e88ca2bad408d90091093bb8d358a4cb29d412daaab6695e86c
        if reviewed {
            self.record_reviews(unit, directory);
        }
        // @lfy def/cli/main.lfy:main#main:main:8940bcdd4260c6e311e87a27ce3497056d6d8956860d16464a361158ed9f48da
        self.rederive_co_owners(unit, directory, &shared, &recorded);
    }

    /// Each co-owner of the unit has its maps re-derived from the files as they are now in the
    /// unit's directory and recorded there, once the unit's maps are recorded.
    ///
    /// The unit's co-owners are the units of its target other than it that have a marker of
    /// their own in a file the recorded maps name, or whose map file names such a file or a
    /// file a map of the unit named before they were recorded. `shared` is every one of those
    /// files and `recorded` the ones the maps recorded here name.
    ///
    /// Nothing else is written for a co-owner: no compiler and no verifier runs for it and no
    /// review record file of its changes, because its code did not change — only where its
    /// regions sit in files another unit rewrote.
    // @lfy def/cli/main.lfy:main#main:main:73b5383aed1ca86a8561196285ac24e93f2aa3b1cf5665ccb3c44c99c8bcfb1f
    // @lfy def/cli/main.lfy:main#main:main:8940bcdd4260c6e311e87a27ce3497056d6d8956860d16464a361158ed9f48da
    // @lfy def/cli/main.lfy:main#main:main:4f7cae6922d2fbb74f58a9c1f34d8e9565a2d72660e593819347a125257e342d
    pub(crate) fn rederive_co_owners(
        &mut self,
        unit: usize,
        directory: &Path,
        shared: &BTreeSet<String>,
        recorded: &BTreeSet<String>,
    ) {
        if shared.is_empty() {
            return;
        }
        let target = self.plan.units[unit].target;
        // The text of each file the maps recorded here name, where a co-owner's own marker may
        // now sit, read once rather than once per unit.
        // @lfy def/cli/main.lfy:main#main:main:73b5383aed1ca86a8561196285ac24e93f2aa3b1cf5665ccb3c44c99c8bcfb1f
        let texts: Vec<String> =
            recorded.iter().filter_map(|output| fs::read_to_string(directory.join(output)).ok()).collect();
        for index in 0..self.plan.units.len() {
            // @lfy def/cli/main.lfy:main#main:main:73b5383aed1ca86a8561196285ac24e93f2aa3b1cf5665ccb3c44c99c8bcfb1f
            if self.plan.units[index].target != target || index == unit {
                continue;
            }
            // @lfy def/cli/main.lfy:main#main:main:8940bcdd4260c6e311e87a27ce3497056d6d8956860d16464a361158ed9f48da
            let was = self.maps_in(index, directory);
            // Its map file names one of the files the unit shares with it, or one of its own
            // markers sits in a file the unit just recorded.
            // @lfy def/cli/main.lfy:main#main:main:73b5383aed1ca86a8561196285ac24e93f2aa3b1cf5665ccb3c44c99c8bcfb1f
            let named = was.iter().any(|map| shared.contains(&map.output));
            let marked = texts.iter().any(|text| {
                generation::parse_markers(text).iter().any(|marker| own_marker(self.workspace(), &self.plan, index, marker))
            });
            if !named && !marked {
                continue;
            }
            // Each co-owner's maps are re-derived once for the unit, in the unit's directory,
            // from the maps its map file there holds, whether or not the outputs they name
            // still exist.
            // @lfy def/cli/main.lfy:main#main:main:8940bcdd4260c6e311e87a27ce3497056d6d8956860d16464a361158ed9f48da
            let plan = self.plan.clone();
            let program = self.program_at(directory);
            let now = generation::rederive(program, &plan, index, &was);
            // @lfy def/cli/main.lfy:main#main:main:94482ac68d310e72da7940331629312470c58e04dbf09548f66eb20565752b91
            self.warn_of_wider_regions(unit, index, &was, &now);
            // Only the co-owner's map file is written for it, in the same directory.
            // @lfy def/cli/main.lfy:main#main:main:4f7cae6922d2fbb74f58a9c1f34d8e9565a2d72660e593819347a125257e342d
            let program = self.program_at(directory);
            if !generation::record(&program.workspace, &plan.units[index], &now) {
                let path = generation::map_file(&program.workspace, &plan.units[index]).display().to_string();
                complain(self.style.err, Some(&path), "could not be written");
                self.worsen(ExitCode::Failure);
            }
        }
    }

    /// A message to standard error for each of a co-owner's own markers whose regions span
    /// more output lines in total than they did in the maps the re-derived ones replace: the
    /// re-derived maps are recorded all the same, and the message names the co-owner, the
    /// unit, and the marker, and says that another unit's lines may now lie in the co-owner's
    /// region.
    // @lfy def/cli/main.lfy:main#main:main:94482ac68d310e72da7940331629312470c58e04dbf09548f66eb20565752b91
    pub(crate) fn warn_of_wider_regions(&mut self, unit: usize, co_owner: usize, was: &[SourceMap], now: &[SourceMap]) {
        let before = self.spans_of(co_owner, was);
        let after = self.spans_of(co_owner, now);
        let stem = self.stem(co_owner);
        let by = self.stem(unit);
        for (marker, lines) in after {
            // A region the maps it replaces did not hold spanned no line in them, so any line
            // of it now is more.
            // @lfy def/cli/main.lfy:main#main:main:94482ac68d310e72da7940331629312470c58e04dbf09548f66eb20565752b91
            if lines <= before.get(&marker).copied().unwrap_or(0) {
                continue;
            }
            let message =
                format!("the region of {marker} grew when {by} was recorded: another unit's lines may now lie in it");
            complain(self.style.err, Some(&stem), &message);
        }
    }

    /// How many output lines the regions of each of one unit's own markers span in total, by
    /// the marker's file, entity, and requirement, which is what names a region.
    // @lfy def/cli/main.lfy:main#main:main:94482ac68d310e72da7940331629312470c58e04dbf09548f66eb20565752b91
    pub(crate) fn spans_of(&self, unit: usize, maps: &[SourceMap]) -> BTreeMap<String, usize> {
        let mut out: BTreeMap<String, usize> = BTreeMap::new();
        for map in maps {
            for marker in &map.markers {
                if !own_marker(self.workspace(), &self.plan, unit, marker) {
                    continue;
                }
                let lines = marker.end.saturating_sub(marker.output_line) + 1;
                *out.entry(region_named(marker)).or_default() += lines;
            }
        }
        out
    }

    /// A unit's own lines: every line of every file its source maps name that lies in no
    /// region of another unit's marker, as the map files in the unit's directory give the
    /// other units' regions; by the file's path relative to that directory.
    // @lfy def/cli/main.lfy:main#main:main:6b6776d9051184c3c3b68bf73d7695e20b6854877e1970d17e5ef917f80dd7c0
    pub(crate) fn own_lines(&self, unit: usize, maps: &[SourceMap], directory: &Path) -> BTreeMap<String, BTreeSet<usize>> {
        // The regions of every other unit's own markers, by the file they are in: a map can
        // name every marker of a file it shares, so only the other unit's own count.
        // @lfy def/cli/main.lfy:main#main:main:6b6776d9051184c3c3b68bf73d7695e20b6854877e1970d17e5ef917f80dd7c0
        let mut regions: Vec<(String, Marker)> = Vec::new();
        for other in 0..self.plan.units.len() {
            let held = &self.plan.units[other];
            if other == unit || held.target != self.plan.units[unit].target || held.file == self.plan.units[unit].file {
                continue;
            }
            for map in self.maps_in(other, directory) {
                for marker in map.markers {
                    if own_marker(self.workspace(), &self.plan, other, &marker) {
                        regions.push((map.output.clone(), marker));
                    }
                }
            }
        }
        let mut out: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
        for map in maps {
            let count = fs::read_to_string(directory.join(&map.output)).map_or(0, |text| text.lines().count());
            // @lfy def/cli/main.lfy:main#main:main:6b6776d9051184c3c3b68bf73d7695e20b6854877e1970d17e5ef917f80dd7c0
            let taken: BTreeSet<usize> = regions
                .iter()
                .filter(|(output, _)| *output == map.output)
                .flat_map(|(_, marker)| marker.output_line..=marker.end.max(marker.output_line))
                .collect();
            let lines = out.entry(map.output.clone()).or_default();
            lines.extend((1..=count).filter(|line| !taken.contains(line)));
        }
        out
    }

    /// The unit gate on a unit's outputs in its directory: one building progress line, then
    /// the target's build command, its test command with stubs allowed, and its lint command,
    /// in that order, each only once the one before it succeeded; a command the target has no
    /// line for is skipped and counts as succeeded. A building progress line then says the
    /// gate passed, or which command failed and its exit code.
    ///
    /// The build or the test command failing fails the gate, with a problem naming the
    /// operation and the exit code, followed by every line of its output that names a location
    /// in a file of the target's output directory. The lint command failing fails it only when
    /// a line of its output names a location that is one of the unit's own lines, and its
    /// problem is followed by those lines; otherwise the lint counts as succeeded.
    // @lfy def/cli/main.lfy:main#main:main:956b0b16d0d8f0352252fcd0a66d8ecc7d1b684b3f54e01150cbd01b317c9999
    // @lfy def/cli/main.lfy:main#main:main:2693766a523314183e6282316ef0592ed35595e8a47c47ed91606d938be00563
    // @lfy def/cli/main.lfy:main#main:main:724e51287e6f04918b01b847bb33591939dd709cfb769cbd4bd7ff3d537c504b
    // @lfy def/cli/main.lfy:main#main:main:3bac0db5561152245649fca44e6f2f4aa275ec221f2d4954e94046bd327e3cab
    // @lfy def/cli/main.lfy:main#main:main:26c73a135ccf59fa42bd380a4eb712ba0f4ccb70d5082c9a3838abe729e4a29f
    // @lfy def/cli/main.lfy:main#main:main:25b8417f9f55800eec5857dab9bbcf5fbbed7096f0f48ce932ccb4e0ea51ac39
    // @lfy def/cli/main.lfy:main#main:main:4502368c3b4408a12c9ea2de6677c21637ddd009b1433f9ca2537f0d00e249e0
    pub(crate) fn unit_gate(&mut self, unit: usize, maps: &[SourceMap], directory: &Path) -> Result<(), String> {
        let target = self.target_of_unit(unit).clone();
        let stem = self.stem(unit);
        // @lfy def/cli/main.lfy:main#main:main:956b0b16d0d8f0352252fcd0a66d8ecc7d1b684b3f54e01150cbd01b317c9999
        self.reporter.report(Step::Building, Some(&stem), &format!("{} build, test, and lint", target.identifier));
        let ran_in = directory.join(&target.output_directory);
        for operation in [Operation::Build, Operation::Test, Operation::Lint] {
            // A command the target has no line for is skipped and counts as succeeded.
            // @lfy def/cli/main.lfy:main#main:main:2693766a523314183e6282316ef0592ed35595e8a47c47ed91606d938be00563
            // @lfy def/cli/main.lfy:main#main:main:ba814337ca5699de23cfbe6d5488f298a354e81440cab6936bf45c6ccc9f2411
            let Some(ran) = run_target_command(&target, operation, directory, Some(&self.running), true) else {
                continue;
            };
            if ran.succeeded() {
                continue;
            }
            // A command that could not run is said so, and the code is failure.
            // @lfy def/cli/main.lfy:main#main:main:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
            self.could_not_run(&ran);
            let failure = format!("the {} command exited with {}", word_of_operation(operation), ran.code);
            let problem = match operation {
                // The lint fails the gate only through a line naming one of the unit's own
                // lines, and its problem names those lines.
                // @lfy def/cli/main.lfy:main#main:main:3bac0db5561152245649fca44e6f2f4aa275ec221f2d4954e94046bd327e3cab
                // @lfy def/cli/main.lfy:main#main:main:26c73a135ccf59fa42bd380a4eb712ba0f4ccb70d5082c9a3838abe729e4a29f
                // @lfy def/cli/main.lfy:main#main:main:c5ad7b82a71dd3043b2097cdddcd5cd97253716022319656a0981d35b1fdb607
                Operation::Lint => {
                    let own = self.own_lines(unit, maps, directory);
                    let named: Vec<&str> = locations_of(&ran.lines, &ran_in, directory)
                        .into_iter()
                        .filter(|(_, path, line)| own.get(path).is_some_and(|lines| lines.contains(line)))
                        .map(|(text, _, _)| text)
                        .collect();
                    if named.is_empty() {
                        continue;
                    }
                    problem_with_lines(&failure, &named)
                }
                // The build or the test failing fails the gate, naming every location in a
                // file of the output directory.
                // @lfy def/cli/main.lfy:main#main:main:724e51287e6f04918b01b847bb33591939dd709cfb769cbd4bd7ff3d537c504b
                // @lfy def/cli/main.lfy:main#main:main:20d39c165928d43ffa93da6970f91339f7c50e254aa218ea87fb80072d87f066
                _ => problem_with_lines(&failure, &lines_in_outputs(&ran, &target, &ran_in, directory)),
            };
            // The building line saying which command failed is printed by the caller, with
            // what follows it, in one turn; it is the problem's first line.
            // @lfy def/cli/main.lfy:main#main:main:4502368c3b4408a12c9ea2de6677c21637ddd009b1433f9ca2537f0d00e249e0
            return Err(problem);
        }
        // @lfy def/cli/main.lfy:main#main:main:25b8417f9f55800eec5857dab9bbcf5fbbed7096f0f48ce932ccb4e0ea51ac39
        self.reporter.report(Step::Building, Some(&stem), "the unit gate passed");
        Ok(())
    }

    /// A command of the target that could not be started: why is printed and logged, and the
    /// code is failure, since a command that could not run is not one that failed.
    // @lfy def/cli/main.lfy:main#main:main:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
    pub(crate) fn could_not_run(&mut self, ran: &Ran) {
        if ran.started {
            return;
        }
        for line in &ran.lines {
            complain_logged(self.style.err, &self.reporter.logs, None, line);
        }
        self.worsen(ExitCode::Failure);
    }

    /// Whether a unit's target has a command with a line for the build operation and one with
    /// a line for the test operation, which is what lets the unit gate stand in for the
    /// compiler.
    // @lfy def/cli/main.lfy:main#main:main:f82d76ff8e23e283093211b99bd7fa4f7edfa0a9f9151cf64b7864379ceec690
    // @lfy def/cli/main.lfy:main#main:main:343acafbb0e354bca79a21aac598241ee5886911dba44bf7b3766bec69054122
    pub(crate) fn builds_and_tests(&self, unit: usize) -> bool {
        let target = self.target_of_unit(unit);
        line_for(target, Operation::Build).is_some() && line_for(target, Operation::Test).is_some()
    }

    /// A failed unit gate printed: one building line saying which command failed and its exit
    /// code, then, when the gate stood in for nothing, the unit rejected with its problem, or
    /// otherwise the problem alone, all in one turn.
    // @lfy def/cli/main.lfy:main#main:main:4502368c3b4408a12c9ea2de6677c21637ddd009b1433f9ca2537f0d00e249e0
    // @lfy def/cli/main.lfy:main#main:main:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
    pub(crate) fn report_gate(&mut self, unit: usize, problem: &str, rejected: bool) {
        let stem = self.stem(unit);
        let _turn = self.reporter.hold();
        // @lfy def/cli/main.lfy:main#main:main:4502368c3b4408a12c9ea2de6677c21637ddd009b1433f9ca2537f0d00e249e0
        self.reporter.report(Step::Building, Some(&stem), problem.lines().next().unwrap_or_default());
        let problems = vec![problem.to_string()];
        if rejected {
            self.report_rejection(unit, &problems);
        } else {
            self.reporter.problems(&problems);
        }
    }

    /// The problems of a rejected verdict: one rejected line for the unit, then its problems
    /// indented below it.
    // @lfy def/cli/main.lfy:main#main:main:c863a527b6e120083837fbd590e228d380afa98faa81f3564fd62d4a4bad94b4
    pub(crate) fn report_rejection(&mut self, unit: usize, problems: &[String]) {
        let stem = self.stem(unit);
        // @lfy def/cli/main.lfy:main#main:main:c863a527b6e120083837fbd590e228d380afa98faa81f3564fd62d4a4bad94b4
        // @lfy def/cli/main.lfy:main#main:main:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
        self.reporter.report_problems(Step::Rejected, Some(&stem), &problems.join("\n"), problems);
    }
}

/// A problem naming what failed, followed by lines of the command's output, one per line.
// @lfy def/cli/main.lfy:main#main:main:20d39c165928d43ffa93da6970f91339f7c50e254aa218ea87fb80072d87f066
pub(crate) fn problem_with_lines(failure: &str, lines: &[&str]) -> String {
    let mut problem = failure.to_string();
    for line in lines {
        problem.push('\n');
        problem.push_str(line.trim_end());
    }
    problem
}

/// Every line of a command's output that names a location in a file of the target's output
/// directory, in the order it wrote them.
// @lfy def/cli/main.lfy:main#main:main:20d39c165928d43ffa93da6970f91339f7c50e254aa218ea87fb80072d87f066
// @lfy def/cli/main.lfy:main#main:main:5d082ae0ac17d428f19ee0894765ffab9b0227cf846ca57511c6451d474a7ff0
pub(crate) fn lines_in_outputs<'r>(ran: &'r Ran, target: &Target, ran_in: &Path, directory: &Path) -> Vec<&'r str> {
    locations_of(&ran.lines, ran_in, directory)
        .into_iter()
        .filter(|(_, path, _)| within(path, &target.output_directory))
        .map(|(text, _, _)| text)
        .collect()
}
