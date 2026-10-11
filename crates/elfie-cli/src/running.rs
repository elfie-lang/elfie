// @lfy def/cli/main.lfy:main
//! What one compile holds while it runs: the plan and the source maps, the outputs a unit
//! owns in a directory, the source a unit's last accepted generation came from, the request
//! made for each batch, and accepting and recording what a batch earned.

use super::*;
use crate::copies::{Kept, KeptUnit, kept_file};
use crate::processes::{Agent, Runs, Running, line_for, run_target_command, word_of_operation};

/// The files under the target's output directory, in the directory the batch ran in, split
/// into the ones that carry a marker for the unit's file and every other one of that listing.
/// Their paths are relative to that directory, which is the root or a copy of it, so an
/// output is named the same either way.
///
/// The second list is what `accept` is given as its `elsewhere`: a file of it earns no source
/// map and none of its markers is checked, and it is there so that a region this unit moved
/// out of a shared file into a file of its own is still found where it went.
// @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
// @lfy def/cli/main.lfy:main#main:main:e6df0c7548d8f79abfca3e578a974cfbbf2949927b4851d9860223b1315c6c27
pub(crate) fn outputs_of(
    workspace: &Workspace,
    plan: &Plan,
    unit: usize,
    directory: &Path,
) -> (Vec<Output>, Vec<Output>) {
    let unit = &plan.units[unit];
    let target = &workspace.targets[unit.target];
    let file = &workspace.files[unit.file].path;
    let mut paths = Vec::new();
    // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
    walk_all(&directory.join(&target.output_directory), &mut paths);
    paths.sort();
    let mut found = Vec::new();
    let mut elsewhere = Vec::new();
    for path in paths {
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let output = Output { path: relative(directory, &path), text };
        // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
        if generation::parse_markers(&output.text).iter().any(|m| &m.file == file) {
            found.push(output);
        } else {
            // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
            elsewhere.push(output);
        }
    }
    (found, elsewhere)
}

/// Whether a marker is one unit's own: it names the unit's file, or it names an entity moved
/// into the unit and the file that declares it.
// @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
pub(crate) fn own_marker(workspace: &Workspace, plan: &Plan, unit: usize, marker: &Marker) -> bool {
    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
    if marker.file == workspace.files[plan.units[unit].file].path {
        return true;
    }
    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
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
// @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
pub(crate) fn own_region_text(workspace: &Workspace, plan: &Plan, unit: usize, text: &str) -> String {
    let comment = &workspace.targets[plan.units[unit].target].marker_comment;
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
    for marker in generation::parse_markers(text) {
        if !own_marker(workspace, plan, unit, &marker) {
            continue;
        }
        let last = marker.end.min(lines.len());
        let first = marker.output_line.saturating_sub(1).min(last);
        for line in &lines[first..last] {
            // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
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
// @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
pub(crate) fn without_marker<'l>(line: &'l str, comment: &str) -> &'l str {
    if comment.is_empty() {
        return line;
    }
    let mut from = 0;
    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
    while let Some(position) = line[from..].find(MARKER) {
        let at = from + position;
        // The comment opener of this marker: the last one before it, which the marker is
        // written after as the guidance says.
        // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
        if let Some(opener) = line[..at].rfind(comment) {
            return line[..opener].trim_end();
        }
        from = at + MARKER.len();
    }
    line
}

/// What every marker begins with, after the target's comment opener.
// @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
pub(crate) const MARKER: &str = "@lfy ";

/// One region named the way a marker spells it: the file, the entity or the line, and the
/// requirement when it answers for one. Two markers of one region spell the same name.
// @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
pub(crate) fn region_named(marker: &Marker) -> String {
    let place = match &marker.entity {
        Some(entity) => entity.clone(),
        None => marker.line.to_string(),
    };
    // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
    match &marker.requirement {
        Some(requirement) => format!("{}:{place}#{requirement}", marker.file),
        None => format!("{}:{place}", marker.file),
    }
}

/// hash-matched source rather than the source its outputs were accepted from.
// @lfy def/cli/main.lfy:main#main:main:c01ce1f2a8e5cd3e6ca0d0a2be31e2356dcb22b0e029c02e817117316275641c
pub(crate) struct Previous {
    /// The revision as git holds it, never lowered.
    // @lfy def/cli/main.lfy:main#main:main:1f3c991d456ca136f454724f3f7fd0dfcb94081ad9e4e7f5f238375155b93e8f
    pub(crate) text: String,
    /// Whether its lowered text hashes to the map but its criterion and test ids do not.
    // @lfy def/cli/main.lfy:main#main:main:f39bd9ab2495264b9736277dccebe0c77551b0d471884faae0d41398da0096fa
    pub(crate) hash_matched: bool,
}

/// The source at the last accepted generation, recovered from git when a recent revision of
/// the file both lowers to the hash the unit's source map holds and has the criterion and test
/// ids that map was recorded for; otherwise the unit's hash-matched source, the most recent
/// revision that lowers to that hash alone, marked as such.
///
/// What is given back is that revision as git holds it, never lowered: `request` lowers the
/// previous text it is handed the way `changes` binds one, so handing it lowered code would
/// lower it twice and strip the criteria from what the compiler reads. The map holds the hash
/// of the lowered text, so the revision the outputs were generated from is the one whose
/// lowered text hashes to it, and a revision that differs only in a comment is the same
/// revision.
///
/// A revision that lowers to the hash but whose ids hash to something else is not what the
/// outputs were accepted from: a rewording leaves the lowered code untouched and changes every
/// id it touched, so only the ids say which revision the outputs answered for.
// @lfy def/cli/main.lfy:main#main:main:c01ce1f2a8e5cd3e6ca0d0a2be31e2356dcb22b0e029c02e817117316275641c
// @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
// @lfy def/cli/main.lfy:main#main:main:f39bd9ab2495264b9736277dccebe0c77551b0d471884faae0d41398da0096fa
pub(crate) fn previous_source(workspace: &Workspace, unit: &Unit) -> Option<Previous> {
    let map = unit.outputs.first()?;
    let file = workspace.files[unit.file].path.clone();
    let log = Process::new("git")
        .args(["log", "--format=%H", "-n", "50", "--", &file])
        .current_dir(&workspace.root)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !log.status.success() {
        return None;
    }
    // The most recent revision that lowers to the map's hash and no more, kept in case none
    // of them has the ids the map was recorded for.
    // @lfy def/cli/main.lfy:main#main:main:f39bd9ab2495264b9736277dccebe0c77551b0d471884faae0d41398da0096fa
    let mut matched: Option<String> = None;
    for revision in String::from_utf8_lossy(&log.stdout).lines() {
        let show = Process::new("git")
            .args(["show", &format!("{revision}:{file}")])
            .current_dir(&workspace.root)
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !show.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&show.stdout).into_owned();
        // The revision is recognized by the hash of its text lowered the way `changes` binds
        // a previous text, since that is what the map holds.
        // @lfy def/cli/main.lfy:main#main:main:c01ce1f2a8e5cd3e6ca0d0a2be31e2356dcb22b0e029c02e817117316275641c
        let Some((lowered, requirements)) = lowered_of(workspace, &file, &text) else { continue };
        // @lfy def/cli/main.lfy:main#main:main:c01ce1f2a8e5cd3e6ca0d0a2be31e2356dcb22b0e029c02e817117316275641c
        if generation::source_hash(&lowered) != map.hash {
            continue;
        }
        // What the request is given for the unit is that source as git holds it, never
        // lowered.
        // @lfy def/cli/main.lfy:main#main:main:1f3c991d456ca136f454724f3f7fd0dfcb94081ad9e4e7f5f238375155b93e8f
        // @lfy def/cli/main.lfy:main#main:main:c01ce1f2a8e5cd3e6ca0d0a2be31e2356dcb22b0e029c02e817117316275641c
        if requirements == map.requirements {
            return Some(Previous { text, hash_matched: false });
        }
        // The lowered code is the map's and the ids are not: this revision only reworded what
        // the outputs answer for, so it is the hash-matched source and not the source at the
        // last accepted generation.
        // @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
        if matched.is_none() {
            // @lfy def/cli/main.lfy:main#main:main:f39bd9ab2495264b9736277dccebe0c77551b0d471884faae0d41398da0096fa
            matched = Some(text);
        }
    }
    // @lfy def/cli/main.lfy:main#main:main:f39bd9ab2495264b9736277dccebe0c77551b0d471884faae0d41398da0096fa
    matched.map(|text| Previous { text, hash_matched: true })
}

/// One file read as something other than what is on disk, bound through the loader and
/// lowered: [`elfie_core::interpret::LoweredFile::text`] of it, the way `changes` binds a
/// previous text, and the SHA-256 of its local criterion and test ids as a source map
/// records them.
// Decision: the ids are read off the lowered file rather than off a plan made for that
// revision, since planning every revision of every unit to recover one text would cost more
// than the recovery is worth; a unit whose entities a plan moves is then not recognized and
// falls back to its hash-matched source, which the criteria already provide for.
// @lfy def/cli/main.lfy:main#main:main:c01ce1f2a8e5cd3e6ca0d0a2be31e2356dcb22b0e029c02e817117316275641c
// @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
pub(crate) fn lowered_of(workspace: &Workspace, path: &str, text: &str) -> Option<(String, String)> {
    // @lfy def/cli/main.lfy:main
    let before = lower(workspace::change(workspace, path, Some(text)));
    // @lfy def/cli/main.lfy:main
    let index = before.workspace.files.iter().position(|file| file.path == path)?;
    let file = &before.files[index];
    let mut ids: Vec<String> = Vec::new();
    // @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
    for node in file.root.nodes() {
        declared_ids(node, &mut ids);
    }
    // @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
    ids.extend(file.criteria.iter().map(|criterion| criterion.id.clone()));
    ids.extend(file.tests.iter().map(|test| test.id.clone()));
    ids.sort();
    // @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
    Some((file.text.clone(), generation::source_hash(&ids.join("\n"))))
}

/// The ids of the criteria and tests of one lowered node and of every node under it.
// @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
pub(crate) fn declared_ids(node: &LoweredNode, out: &mut Vec<String>) {
    out.extend(node.criteria.iter().map(|criterion| criterion.id.clone()));
    out.extend(node.tests.iter().map(|test| test.id.clone()));
    for child in node.nodes() {
        declared_ids(child, out);
    }
}

/// What one compile is doing: the plan, the source maps as they are recorded, the counts
/// the finished line reports, and the units a stopped batch took down with it. The maps a
/// batch earns are held beside these until nothing has violated what was asked; only then
/// do they replace the units' here and reach the disk.
// @lfy def/cli/main.lfy:main
pub(crate) struct Run {
    /// The workspace lowered: what the plan, the requests, and the reviews are read from. The
    /// compile owns it, because a violated global review plans its units again and the plan is
    /// made from a workspace loaded afresh.
    // @lfy def/cli/main.lfy:main
    pub(crate) program: Program,
    pub(crate) plan: Plan,
    pub(crate) maps: Vec<SourceMap>,
    pub(crate) reporter: Reporter,
    /// What running the compiler and the verifier takes, held apart so that a batch streams
    /// its agents while the lock on the rest of this is free for the other batches.
    // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
    pub(crate) agent: Agent,
    pub(crate) json: bool,
    /// Whether each stream is painted.
    // @lfy def/cli/main.lfy:main
    pub(crate) style: Paint,
    pub(crate) continuing: bool,
    /// The command elfie.json names under `verifier`; nothing verifies a batch without one.
    // @lfy def/cli/main.lfy:main#main:main:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
    pub(crate) verifier: Option<String>,
    /// --no-verify: no verifier runs and nothing is reviewed.
    // @lfy def/cli/main.lfy:main#main:main:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
    pub(crate) no_verify: bool,
    /// --accept: no compiler is run, and no review record file changes.
    // @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
    pub(crate) accept_only: bool,
    /// --full: every criterion and test of a batch is reviewed again, carrying nothing.
    // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
    pub(crate) full: bool,
    /// What the last review of each batch found, by batch identifier, so that the review
    /// records of its units are written when its source maps are recorded.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    pub(crate) reviewed: BTreeMap<String, Reviewed>,
    /// The process groups each batch has running, by batch identifier, so that a stopped
    /// compile or a stopped batch stops them and everything they started.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    pub(crate) running: BTreeMap<String, Arc<Running>>,
    pub(crate) halted: bool,
    /// Whether a batch of the round under way was accepted and its source maps recorded, so
    /// that the global review runs even when every unit was counted by an earlier round.
    // @lfy def/cli/main.lfy:main
    pub(crate) accepted_batch: bool,
    pub(crate) accepted: usize,
    pub(crate) rejected: usize,
    pub(crate) blocked: usize,
    pub(crate) failed: usize,
    pub(crate) stopped: BTreeSet<usize>,
    /// The stems accepted and written back, so that a unit accepted again after a review
    /// sent its batch back to the compiler is counted once, and a unit compiled again in a
    /// later round of the same compile is too.
    // @lfy def/cli/main.lfy:main
    pub(crate) recorded: BTreeSet<String>,
    pub(crate) incomplete: Vec<String>,
    pub(crate) code: ExitCode,
}

/// What the last review of one batch found: the reviews of its last run of the verifier, and
/// the hashes and carried verdicts of the review request that run was made with. The review
/// records of the batch's units are written from these when its source maps are recorded.
// @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
pub(crate) struct Reviewed {
    /// The reviews of the batch's last verifier run.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    pub(crate) reviews: Vec<Review>,
    /// The hash that run's review request gave each id.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    pub(crate) hashes: BTreeMap<String, String>,
    /// The verdicts that review request carried.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    pub(crate) carried: Vec<ReviewRecord>,
}

impl Run {
    // @lfy def/cli/main.lfy:main
    pub(crate) fn new(program: Program, plan: Plan, maps: Vec<SourceMap>, reporter: Reporter, invocation: &Invocation) -> Run {
        let verifier = verifier_command(&program.workspace.root);
        // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
        let agent = Agent::new(
            invocation.flag("json"),
            Paint::of(invocation),
            reporter.logs.clone(),
            &program.workspace.root,
        );
        Run {
            program,
            plan,
            maps,
            reporter,
            // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
            agent,
            json: invocation.flag("json"),
            // @lfy def/cli/main.lfy:main
            style: Paint::of(invocation),
            continuing: invocation.flag("continue"),
            // @lfy def/cli/main.lfy:main
            verifier,
            no_verify: invocation.flag("no-verify"),
            // @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
            accept_only: invocation.flag("accept"),
            // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
            full: invocation.flag("full"),
            // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
            reviewed: BTreeMap::new(),
            // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
            running: BTreeMap::new(),
            halted: false,
            accepted_batch: false,
            accepted: 0,
            rejected: 0,
            blocked: 0,
            failed: 0,
            stopped: BTreeSet::new(),
            recorded: BTreeSet::new(),
            incomplete: Vec::new(),
            code: ExitCode::Success,
        }
    }

    /// The workspace the plan was made from.
    // @lfy def/cli/main.lfy:main
    pub(crate) fn workspace(&self) -> &Workspace {
        &self.program.workspace
    }

    /// The units a violated global review planned, compiled once more in the same compile:
    /// the program and the plan are replaced and the batches stopped by the round before are
    /// forgotten, while every count the finished line reports is kept.
    // @lfy def/cli/main.lfy:main
    pub(crate) fn replan(&mut self, program: Program, plan: Plan, total: usize) {
        self.program = program;
        self.plan = plan;
        self.reporter.total = total;
        self.halted = false;
        self.accepted_batch = false;
        self.stopped.clear();
        self.incomplete.clear();
        // The reviews of the round before answered for the plan it was made from, so nothing
        // of them is carried into the records of a unit compiled once more.
        // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
        self.reviewed.clear();
    }

    /// The code so far, or a new one when it is more severe.
    // @lfy def/cli/main.lfy:main
    pub(crate) fn worsen(&mut self, code: ExitCode) {
        if code.rank() > self.code.rank() {
            self.code = code;
        }
    }

    /// A stopped batch stops only the batches that depend on one of its units; without
    /// --continue it stops the compile.
    // @lfy def/cli/main.lfy:main#main:main:fef239376f4395cd11e23eb728a352b07322301b631d9069086add1723844c11
    pub(crate) fn stop(&mut self, batch: &Batch) {
        // @lfy def/cli/main.lfy:main#main:main:df6beb5b55efabb4bf57333ff81f1f65c615c2988fc6090856921440a10c3639
        self.incomplete.push(batch.identifier.clone());
        self.stopped.extend(batch.units.iter().copied());
        // @lfy def/cli/main.lfy:main#main:main:b4d24a6285bcfe6faf8fa684cd04234bc339a6acb61cf4c5597af955903b8ba1
        // With --continue only the batches that depend on one of its units are stopped, and
        // the batches that depend on none of them still run.
        // @lfy def/cli/main.lfy:main#main:main:2f8c90c5e12c7f100f1b5428e6c30011d648af8d88b9a3ce42d079de42b15071
        // @lfy def/cli/main.lfy:main#main:main:b4bf8bd2d74cf6932c3ed9afe7c76d862161f90df8e2ae4a6e2cb1728ba3e9b8
        if !self.continuing {
            self.halted = true;
            // The compile stops: every run of the compiler, the verifier, or a command of
            // the target still running for a batch is stopped, with every process of its
            // group, and what it gives is ignored, since a batch whose run was stopped
            // merges and records nothing.
            // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
            self.stop_every_run();
        }
    }

    /// The process groups one batch has running, made the first time the batch runs
    /// something.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    pub(crate) fn running_of(&mut self, label: &str) -> Arc<Running> {
        // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
        self.running.entry(label.to_string()).or_default().clone()
    }

    /// Every run of the compiler, the verifier, or a command of the target still running for
    /// one batch stopped, with every process of its group.
    // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
    pub(crate) fn stop_running(&self, label: &str) {
        // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
        if let Some(running) = self.running.get(label) {
            running.stop();
        }
    }

    /// Every run still running for any batch stopped, with every process of its group: what
    /// a compile that stops does before it returns.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    pub(crate) fn stop_every_run(&self) {
        // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
        for running in self.running.values() {
            running.stop();
        }
    }

    /// The batches of the plan a compile runs, in plan order: every one, or only those of the
    /// target --target names.
    // @lfy def/cli/main.lfy:main#main:main:ab0e6c732c90fb6b7fa9232e9b5f894e7f913233bb84f30a1d08a14830314820
    pub(crate) fn batches_for(&self, target: Option<&str>) -> Vec<usize> {
        (0..self.plan.batches.len())
            .filter(|&index| {
                // @lfy def/cli/main.lfy:main#main:main:ab0e6c732c90fb6b7fa9232e9b5f894e7f913233bb84f30a1d08a14830314820
                target.is_none_or(|name| batch_target(self.workspace(), &self.plan, &self.plan.batches[index]) == name)
            })
            .collect()
    }

    /// Whether a batch holds a unit depending on one of a batch that did not complete.
    // @lfy def/cli/main.lfy:main#main:main:fef239376f4395cd11e23eb728a352b07322301b631d9069086add1723844c11
    pub(crate) fn depends_on_stopped(&self, batch: &Batch) -> bool {
        batch
            .units
            .iter()
            .any(|&unit| self.plan.units[unit].dependencies.iter().any(|d| self.stopped.contains(d)))
    }

    /// The directory a batch's requests, questions, and reasons are written to:
    /// `elfie-requests` under the root, never under an output directory.
    // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
    pub(crate) fn requests_directory(&self) -> PathBuf {
        requests_directory(&self.workspace().root)
    }

    /// Writes `elfie-requests/<batch>.<suffix>` under the root.
    // @lfy def/cli/main.lfy:main#main:main:b01f4990d763a4c86c9d8bc4995bc1c02eebe03f8a9f36d24f234cfd4e6042df
    // @lfy def/cli/main.lfy:main#main:main:0e04fca1d5a1398b0203e9dfa3d58b7324f1ef742ad13c350264b5b958456075
    pub(crate) fn write_note(&self, batch: &Batch, suffix: &str, text: &str) -> Option<PathBuf> {
        let directory = self.requests_directory();
        let path = directory.join(format!("{}.{suffix}", file_name_of(&batch.identifier)));
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

    /// The answer a person wrote below the question in
    /// `elfie-requests/<batch>.question.md`, when there is one.
    // @lfy def/cli/main.lfy:main#main:main:760ac9394baad44b62a3c96e58e305792fb43b2fe0dc7ab8510183a1ad12b070
    pub(crate) fn answer_of(&self, batch: &Batch) -> Option<String> {
        let directory = self.requests_directory();
        let path = directory.join(format!("{}.question.md", file_name_of(&batch.identifier)));
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

    /// The request of one batch, with its existing outputs read from the batch's directory,
    /// the source each unit's outputs were generated from where git still holds it, and the
    /// answer to a question asked before appended.
    ///
    /// `hosted` is whether the request is for a compiler run in a hosted session of this
    /// command's agent server: true for the request handed to a compiler command, false for
    /// the one written out for a compiler run by hand.
    // @lfy def/cli/main.lfy:main
    pub(crate) fn request_of(&self, batch: &Batch, directory: &Path, hosted: bool, kept: Option<&Kept>) -> Request {
        let existing: Vec<Output> = batch
            .units
            .iter()
            .flat_map(|&unit| self.plan.units[unit].outputs.iter())
            .filter_map(|map| {
                fs::read_to_string(directory.join(&map.output))
                    .ok()
                    .map(|text| Output { path: map.output.clone(), text })
            })
            .collect();
        // @lfy def/cli/main.lfy:main#main:main:c01ce1f2a8e5cd3e6ca0d0a2be31e2356dcb22b0e029c02e817117316275641c
        let mut previous = BTreeMap::new();
        // The path of each unit whose previous text is only its hash-matched source: it is
        // given exactly when that text is the hash-matched source and not the source at the
        // unit's last accepted generation.
        // @lfy def/cli/main.lfy:main#main:main:44d118a88d1e7a297fd7918629fa1c6c140077672079734436b5b02679cb1207
        let mut hash_matched: Vec<String> = Vec::new();
        for &index in &batch.units {
            let unit = &self.plan.units[index];
            if let Some(found) = previous_source(self.workspace(), unit) {
                let path = self.workspace().files[unit.file].path.clone();
                // @lfy def/cli/main.lfy:main#main:main:44d118a88d1e7a297fd7918629fa1c6c140077672079734436b5b02679cb1207
                if found.hash_matched {
                    // @lfy def/cli/main.lfy:main#main:main:f39bd9ab2495264b9736277dccebe0c77551b0d471884faae0d41398da0096fa
                    hash_matched.push(path.clone());
                }
                previous.insert(path, found.text);
            }
        }
        // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
        // @lfy def/cli/main.lfy:main#main:main:de437a586cddefeefabed78d855e65916788f25c33c6c0d9b7286be02d199ce5
        // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
        let mut request =
            generation::request(&self.program, &self.plan, batch, &existing, &previous, hosted, &hash_matched);
        // The person answers by editing the definitions, or by writing the answer below
        // the question, which the next compile of the batch appends to its instructions.
        // @lfy def/cli/main.lfy:main#main:main:760ac9394baad44b62a3c96e58e305792fb43b2fe0dc7ab8510183a1ad12b070
        if let Some(answer) = self.answer_of(batch) {
            request.instructions.push_str(&format!("\n\n## The answer to the question asked before\n\n{answer}\n"));
        }
        // A batch that took its kept work is told so: the outputs it is handed hold work a run
        // rejected and that was never accepted, and the problems that rejection named follow,
        // one per line.
        // @lfy def/cli/main.lfy:main#main:main:42f4bccc722123748448fab03bfe2e9f1e2ec7530f09efd662aa8ce9f1cb26a2
        if let Some(kept) = kept {
            // @lfy def/cli/main.lfy:main#main:main:42f4bccc722123748448fab03bfe2e9f1e2ec7530f09efd662aa8ce9f1cb26a2
            request.instructions.push_str(
                "\n\nIts existing outputs hold work a run rejected and that was never accepted.\n",
            );
            // @lfy def/cli/main.lfy:main#main:main:42f4bccc722123748448fab03bfe2e9f1e2ec7530f09efd662aa8ce9f1cb26a2
            for problem in &kept.problems {
                request.instructions.push_str(&format!("{problem}\n"));
            }
        }
        request
    }

    /// The kept work the batch would ask for: its target and, for each unit in batch order,
    /// its stem, the SHA-256 of [`Unit::text`], and the SHA-256 of its local criterion and
    /// test ids. What a file on disk is compared with before anything of it is taken.
    // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    pub(crate) fn asked_kept(&self, batch: &Batch) -> Kept {
        // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
        let units = batch
            .units
            .iter()
            .map(|&index| {
                let unit = &self.plan.units[index];
                KeptUnit {
                    stem: unit.stem.clone(),
                    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
                    hash: generation::source_hash(&unit.text),
                    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
                    requirements: generation::source_hash(&requirement_ids_of(&self.program, unit).join("\n")),
                }
            })
            .collect();
        Kept {
            // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
            target: batch_target(self.workspace(), &self.plan, batch).to_string(),
            units,
            problems: Vec::new(),
            files: Vec::new(),
        }
    }

    /// Whether every output of the batch's units has the unit's own region text that
    /// [`SourceMap::text`] of its map holds: the outputs on disk are exactly the ones the last
    /// acceptance recorded, so nothing of them has to be generated again.
    ///
    /// Both sides are read in the batch's directory: the output, and the unit's maps from its
    /// map file there, so maps another unit's acceptance re-derived before the copy was made,
    /// or in place before this comparison, count. A unit with no map there has nothing on disk
    /// that stands, so it never holds, and neither does a map recorded before the text hash
    /// was, whose empty hash no text equals.
    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
    // @lfy def/cli/main.lfy:main#main:main:c8973e9e2d07a79f390133eefde25b3157ae35c014f978eb58e3a63d4f7eada0
    pub(crate) fn outputs_stand(&self, batch: &Batch, directory: &Path) -> bool {
        // @lfy def/cli/main.lfy:main#main:main:c8973e9e2d07a79f390133eefde25b3157ae35c014f978eb58e3a63d4f7eada0
        batch.units.iter().all(|&index| {
            // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
            let maps = self.maps_in(index, directory);
            !maps.is_empty()
                && maps.iter().all(|map| {
                    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
                    fs::read_to_string(directory.join(&map.output)).is_ok_and(|text| {
                        // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
                        let own = own_region_text(self.workspace(), &self.plan, index, &text);
                        generation::source_hash(&own) == map.text
                    })
                })
        })
    }

    /// The source maps of one unit as the map file in a directory holds them, as recorded:
    /// whether or not the outputs they name still exist, which is how `sourceMapsOf` reads a
    /// map file apart from that.
    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
    pub(crate) fn maps_in(&self, unit: usize, directory: &Path) -> Vec<SourceMap> {
        // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
        generation::read_source_maps(&self.map_file_in(unit, directory))
    }

    /// The map file of one unit under a directory, which is the root or a copy of it, so a
    /// unit's maps are named the same either way.
    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
    pub(crate) fn map_file_in(&self, unit: usize, directory: &Path) -> PathBuf {
        let unit = &self.plan.units[unit];
        // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
        directory
            .join(MAPS)
            .join(&self.workspace().targets[unit.target].identifier)
            .join(format!("{}.json", unit.stem))
    }

    /// The markers of each unit's reworded criteria and tests rewritten to the ids they have
    /// now, in the outputs on disk in the batch's directory, before a request is made for the
    /// batch.
    ///
    /// Nothing is rewritten for a unit whose source at its last accepted generation git no
    /// longer holds, since nothing then says which id replaces which. One rewording progress
    /// line per unit counts its reworded criteria and tests and the outputs written.
    // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
    // @lfy def/cli/main.lfy:main#main:main:fa18c20576f142bd6c6ed82dce02b8ba5a122973c56fb0aae68b0c40a74c201c
    pub(crate) fn reword_outputs(&mut self, batch: &Batch, directory: &Path) {
        for &index in &batch.units {
            let unit = self.plan.units[index].clone();
            // Only the source at the unit's last accepted generation says which id replaces
            // which; its hash-matched source answered for other ids altogether.
            // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
            let Some(previous) = previous_source(self.workspace(), &unit).filter(|found| !found.hash_matched) else {
                continue;
            };
            // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
            let rewordings = generation::reworded(&self.program, &unit, &previous.text);
            if rewordings.is_empty() {
                continue;
            }
            // The id each marker names now, by the id it names on disk.
            // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
            let ids: BTreeMap<String, String> = rewordings
                .iter()
                .map(|rewording| (rewording.replaces.clone(), rewording.id.clone()))
                .collect();
            // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
            let (outputs, _) = outputs_of(self.workspace(), &self.plan, index, directory);
            // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
            let renamed = generation::rename(self.workspace(), &self.plan, index, &outputs, &ids);
            let mut written = 0;
            for output in &renamed {
                let path = directory.join(&output.path);
                // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
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
                // @lfy def/cli/main.lfy:main#main:main:fa18c20576f142bd6c6ed82dce02b8ba5a122973c56fb0aae68b0c40a74c201c
                let message = format!("{} reworded, {written} outputs written", rewordings.len());
                self.reporter.report(Step::Rewording, Some(&batch.identifier), Some(&unit.stem), &message);
            }
        }
    }

    /// The target's build command and then its test command run on a batch in its directory,
    /// in place of the compiler, with a building progress line before them: nothing when both
    /// succeeded, and otherwise which command failed and its exit code.
    ///
    /// It takes the compile locked rather than borrowed, since the lock is free while a
    /// command runs: that is what lets the other batches run theirs at the same time.
    // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
    // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
    pub(crate) fn build_and_test(run: &Mutex<Run>, batch: &Batch, directory: &Path, runs: &Runs) -> Result<(), String> {
        let (target, logs) = {
            let mut state = locked(run);
            let Some(target) = state.target_of(batch) else {
                return Err("the batch has no target".to_string());
            };
            // A building progress line is printed before the build command runs.
            // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
            let line = format!("{} build and then test", target.identifier);
            state.reporter.report(Step::Building, Some(&batch.identifier), None, &line);
            (target, state.reporter.logs.clone())
        };
        // The build command runs first, and the test command only once it succeeded.
        // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
        for operation in [Operation::Build, Operation::Test] {
            // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
            let Some(code) = run_target_command(&target, operation, directory, &logs, Some(&runs.running)) else {
                // @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
                return Err(format!("the target has no {} command", word_of_operation(operation)));
            };
            // A command succeeds when its exit code is 0.
            // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
            if code != 0 {
                // @lfy def/cli/main.lfy:main#main:main:9253f376492f06a33bd0110d5f3f29c04bffd6a3b5a3a922f56cb3f2d9a668e9
                return Err(format!("the {} command exited with {code}", word_of_operation(operation)));
            }
        }
        Ok(())
    }

    /// The target of a batch, as the workspace declares it.
    // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
    pub(crate) fn target_of(&self, batch: &Batch) -> Option<Target> {
        let name = batch_target(self.workspace(), &self.plan, batch).to_string();
        // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
        self.workspace().targets.iter().find(|one| one.identifier == name).cloned()
    }

    /// Whether the batch's target has a command with a line for the build operation and one
    /// with a line for the test operation, which is what lets them stand in for the compiler.
    // @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
    pub(crate) fn builds_and_tests(&self, batch: &Batch) -> bool {
        // @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
        self.target_of(batch).is_some_and(|target| {
            line_for(&target, Operation::Build).is_some() && line_for(&target, Operation::Test).is_some()
        })
    }

    /// For each unit of the batch, `accept` runs on the files under the target's output
    /// directory in the batch's directory that carry a marker for the unit.
    // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
    pub(crate) fn verdicts_of(&mut self, batch: &Batch, request: &Request, directory: &Path) -> Vec<Verdict> {
        let mut verdicts = Vec::new();
        for &unit in &batch.units {
            let stem = self.plan.units[unit].stem.clone();
            let reason = self.plan.units[unit].reason.map_or_else(|| "up to date".to_string(), |r| r.as_str().to_string());
            self.reporter.report(Step::Checking, Some(&batch.identifier), Some(&stem), &reason);
            // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
            let (outputs, elsewhere) = outputs_of(self.workspace(), &self.plan, unit, directory);
            // The source maps recorded for the target's other units, which say which outputs
            // already belong to another unit.
            // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
            let recorded = self.recorded_elsewhere(unit, directory);
            verdicts.push(generation::accept(
                &self.program,
                &self.plan,
                request,
                unit,
                &outputs,
                &recorded,
                &elsewhere,
            ));
        }
        verdicts
    }

    /// The source maps recorded for the other units of one unit's target: what `accept` reads
    /// to see whether an output already belongs to another unit. The unit's own are left out,
    /// since writing its own output again is what it is being accepted for.
    ///
    /// They are read from the map files in the batch's directory, as recorded, so a map
    /// another unit's acceptance re-derived is what this one is checked against.
    // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
    pub(crate) fn recorded_elsewhere(&self, unit: usize, directory: &Path) -> Vec<SourceMap> {
        let target = self.workspace().targets[self.plan.units[unit].target].identifier.clone();
        let source = self.workspace().files[self.plan.units[unit].file].path.clone();
        let mut out: Vec<SourceMap> = Vec::new();
        // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
        for other in 0..self.plan.units.len() {
            let unit = &self.plan.units[other];
            if self.workspace().targets[unit.target].identifier != target
                || self.workspace().files[unit.file].path == source
            {
                continue;
            }
            // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
            out.extend(self.maps_in(other, directory));
        }
        out
    }

    /// One unit's outputs written back in the batch's directory and the unit counted as
    /// done. The source maps [`generation::accept`] derived replace the unit's in the list
    /// held for the batch; nothing reaches a map file until that list is recorded.
    // @lfy def/cli/main.lfy:main#main:main:c0ff42fd47c552fedc4e3213d3ec87968ae59279faec3d132caf6866820027d7
    pub(crate) fn write_back(
        &mut self,
        unit: usize,
        verdict: &Verdict,
        held: &mut Vec<SourceMap>,
        directory: &Path,
    ) -> io::Result<()> {
        for output in &verdict.outputs {
            // The outputs are written back with their markers normalized.
            // @lfy def/cli/main.lfy:main#main:main:c0ff42fd47c552fedc4e3213d3ec87968ae59279faec3d132caf6866820027d7
            // @lfy def/cli/main.lfy:main#main:main:bba3021a7366935ae1491608251547392369c5d323bf75ec2cab5bbbd9981a54
            let path = directory.join(&output.path);
            // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, &output.text)?;
        }
        let target = self.workspace().targets[self.plan.units[unit].target].identifier.clone();
        let source = self.workspace().files[self.plan.units[unit].file].path.clone();
        held.retain(|map| !(map.target == target && map.source == source));
        held.extend(verdict.source_maps.iter().cloned());
        // A unit accepted again, because its reviews sent its batch back to the compiler,
        // is counted once.
        // @lfy def/cli/main.lfy:main#main:main:081257eb508c08a673bc5c569b3099485967e6b85b434c98d303c2de2d9958c5
        if self.recorded.insert(self.plan.units[unit].stem.clone()) {
            self.accepted += 1;
            self.reporter.done = self.accepted;
        }
        Ok(())
    }

    /// Every unit of an accepted batch written back in the batch's directory, and the source
    /// maps its verdicts earned held beside the recorded ones: the verifier is pointed at
    /// them, and they are recorded only once nothing has violated what was asked.
    // @lfy def/cli/main.lfy:main#main:main:c0ff42fd47c552fedc4e3213d3ec87968ae59279faec3d132caf6866820027d7
    pub(crate) fn hold_batch(&mut self, batch: &Batch, verdicts: &[Verdict], directory: &Path) -> Vec<SourceMap> {
        let mut held = self.maps.clone();
        for (&unit, verdict) in batch.units.iter().zip(verdicts) {
            let stem = self.plan.units[unit].stem.clone();
            if let Err(error) = self.write_back(unit, verdict, &mut held, directory) {
                complain(self.style.err, Some(&stem), &error.to_string());
                self.worsen(ExitCode::Failure);
                continue;
            }
            let written = format!("{} outputs", verdict.outputs.len());
            self.reporter.report(Step::Accepted, Some(&batch.identifier), Some(&stem), &written);
        }
        held
    }

    /// The source maps one batch held recorded: each of its units' are recorded in its own
    /// map file, so recording one unit leaves every other map file byte for byte as it was.
    ///
    /// Only the batch's own units are taken from what it held, and only they are replaced
    /// here, because a batch running beside it earned its own maps from a list taken before
    /// these and would otherwise lose them.
    // @lfy def/cli/main.lfy:main#main:main:5841e3d28e75b69d42ed2ba411b34aa9429ca7415424978b7b95493fed283150
    // @lfy def/cli/main.lfy:main#main:main:547a317d986ea3f6e5aa1c3f7159cf0286efabfa3aa41ee361f5f5c8c9611f52
    // @lfy def/cli/main.lfy:main#main:main:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
    pub(crate) fn record_maps(&mut self, batch: &Batch, held: &[SourceMap], on_build: bool) {
        // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
        self.accepted_batch = true;
        let mut unwritten: Vec<String> = Vec::new();
        // The files the batch's units' maps named before they were recorded, read off their
        // map files in the root: a unit whose map file names one of them is a co-owner too,
        // since this batch has just stopped writing a file it used to share with it.
        // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
        let mut shared: BTreeSet<String> = BTreeSet::new();
        for &index in &batch.units {
            let workspace = &self.program.workspace;
            let unit = &self.plan.units[index];
            // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
            for map in generation::read_source_maps(&generation::map_file(workspace, unit)) {
                shared.insert(map.output);
            }
        }
        // The files the maps recorded here name, which are where another unit's marker may now
        // sit inside a region of this batch's.
        // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
        let mut recorded_outputs: BTreeSet<String> = BTreeSet::new();
        for &index in &batch.units {
            let unit = &self.plan.units[index];
            // Only a unit written back in this compile has anything new to record.
            // @lfy def/cli/main.lfy:main#main:main:a64dbb7ffadb4cffc5916067dd80d88bf16cb8db4d6aadf2d4406ffe3d0553e4
            if !self.recorded.contains(&unit.stem) {
                continue;
            }
            let workspace = &self.program.workspace;
            let target = workspace.targets[unit.target].identifier.clone();
            let source = workspace.files[unit.file].path.clone();
            let mine: Vec<SourceMap> =
                held.iter().filter(|map| map.target == target && map.source == source).cloned().collect();
            // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
            recorded_outputs.extend(mine.iter().map(|map| map.output.clone()));
            // @lfy def/cli/main.lfy:main#main:main:547a317d986ea3f6e5aa1c3f7159cf0286efabfa3aa41ee361f5f5c8c9611f52
            self.maps.retain(|map| !(map.target == target && map.source == source));
            self.maps.extend(mine.iter().cloned());
            let workspace = &self.program.workspace;
            let unit = &self.plan.units[index];
            // @lfy def/cli/main.lfy:main#main:main:5841e3d28e75b69d42ed2ba411b34aa9429ca7415424978b7b95493fed283150
            if !generation::record(workspace, unit, &mine) {
                unwritten.push(generation::map_file(workspace, unit).display().to_string());
            }
        }
        // The maps of every unit of the batch that has them recorded are recorded: each
        // co-owner's maps are re-derived from the files as they are now, so a unit that shares
        // an output with the batch is told where its regions went.
        // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
        shared.extend(recorded_outputs.iter().cloned());
        self.rederive_co_owners(batch, &shared, &recorded_outputs);
        // The source maps of the batch are recorded: its review records are written beside
        // them, so that the next compile carries the verdicts of what it did not change. A
        // batch accepted on its build and test had no verifier run on it and no review
        // request made for it, so no review record file of its units changes.
        // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
        // @lfy def/cli/main.lfy:main#main:main:b82042daa1550092c2eeae599eba3e1056e1c9987779953b6b11500318102df7
        if !on_build {
            // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
            self.record_reviews(batch);
        }
        // The work a rejected run of the batch kept is no longer anything to take: its
        // outputs are recorded now, whatever the batch's directory was.
        // @lfy def/cli/main.lfy:main#main:main:890ae3d7c53cdb8c1d8297b9a9fc00b295121ba77bafc958ef442ac72d599abd
        let path = kept_file(&self.workspace().root, &batch.identifier);
        // @lfy def/cli/main.lfy:main#main:main:890ae3d7c53cdb8c1d8297b9a9fc00b295121ba77bafc958ef442ac72d599abd
        if let Err(error) = fs::remove_file(&path)
            && error.kind() != io::ErrorKind::NotFound
        {
            complain(self.style.err, Some(&path.display().to_string()), &error.to_string());
        }
        for path in unwritten {
            complain(self.style.err, Some(&path), "could not be written");
            self.worsen(ExitCode::Failure);
        }
    }

    /// Each co-owner of the batch has its maps re-derived from the files as they are now and
    /// recorded, once the maps of every unit of the batch that has them recorded are recorded
    /// in the root.
    ///
    /// The batch's co-owners are the units of its target that are not of the batch and that
    /// have a marker of their own in a file the recorded maps name, or whose map file names
    /// such a file or a file a map of the batch's units named before they were recorded.
    /// `shared` is every one of those files and `recorded` the ones the maps recorded here
    /// name.
    ///
    /// Nothing else is written for a co-owner: no compiler and no verifier runs for it and no
    /// review record file of its changes, because its code did not change — only where its
    /// regions sit in files another unit rewrote.
    // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
    // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
    // @lfy def/cli/main.lfy:main#main:main:14b01e32487ce04075ecbde039b065c7ad9311e9e5e8cb3e8c5117b41e849f61
    pub(crate) fn rederive_co_owners(&mut self, batch: &Batch, shared: &BTreeSet<String>, recorded: &BTreeSet<String>) {
        if shared.is_empty() {
            return;
        }
        let root = self.workspace().root.clone();
        let target = batch_target(self.workspace(), &self.plan, batch).to_string();
        // The text of each file the maps recorded here name, where a co-owner's own marker may
        // now sit, read once rather than once per unit.
        // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
        let texts: Vec<String> =
            recorded.iter().filter_map(|output| fs::read_to_string(root.join(output)).ok()).collect();
        // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
        for index in 0..self.plan.units.len() {
            // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
            if self.workspace().targets[self.plan.units[index].target].identifier != target
                || batch.units.contains(&index)
            {
                continue;
            }
            let workspace = &self.program.workspace;
            // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
            let was = generation::read_source_maps(&generation::map_file(workspace, &self.plan.units[index]));
            // Its map file names one of the files the batch shares with it, or one of its own
            // markers sits in a file the batch just recorded.
            // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
            let named = was.iter().any(|map| shared.contains(&map.output));
            // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
            let marked = texts.iter().any(|text| {
                generation::parse_markers(text)
                    .iter()
                    .any(|marker| own_marker(self.workspace(), &self.plan, index, marker))
            });
            if !named && !marked {
                continue;
            }
            // Each co-owner's maps are re-derived once for the batch, from the maps its map
            // file holds, whether or not the outputs they name still exist.
            // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
            let now = generation::rederive(&self.program, &self.plan, index, &was);
            // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
            self.warn_of_wider_regions(batch, index, &was, &now);
            let workspace = &self.program.workspace;
            let unit = &self.plan.units[index];
            // Only the co-owner's map file is written for it.
            // @lfy def/cli/main.lfy:main#main:main:14b01e32487ce04075ecbde039b065c7ad9311e9e5e8cb3e8c5117b41e849f61
            // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
            if !generation::record(workspace, unit, &now) {
                let path = generation::map_file(workspace, unit).display().to_string();
                complain(self.style.err, Some(&path), "could not be written");
                self.worsen(ExitCode::Failure);
                continue;
            }
            // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
            let (target, source) = {
                let unit = &self.plan.units[index];
                (
                    self.workspace().targets[unit.target].identifier.clone(),
                    self.workspace().files[unit.file].path.clone(),
                )
            };
            // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
            self.maps.retain(|map| !(map.target == target && map.source == source));
            self.maps.extend(now);
        }
    }

    /// A message to standard error for each of a co-owner's own markers whose regions span
    /// more output lines in total than they did in the maps the re-derived ones replace: the
    /// re-derived maps are recorded all the same, and the message says that another unit's
    /// lines may now lie in the co-owner's region.
    // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
    pub(crate) fn warn_of_wider_regions(&mut self, batch: &Batch, unit: usize, was: &[SourceMap], now: &[SourceMap]) {
        // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
        let before = self.spans_of(unit, was);
        // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
        let after = self.spans_of(unit, now);
        let stem = self.plan.units[unit].stem.clone();
        for (marker, lines) in after {
            // A region the maps it replaces did not hold spanned no line in them, so any line
            // of it now is more.
            // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
            if lines <= before.get(&marker).copied().unwrap_or(0) {
                continue;
            }
            // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
            let message = format!(
                "the region of {marker} grew when {} was recorded: another unit's lines may now lie in it",
                batch.identifier
            );
            // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
            complain(self.style.err, Some(&stem), &message);
        }
    }

    /// How many output lines the regions of each of one unit's own markers span in total, by
    /// the marker's file, entity, and requirement, which is what names a region.
    // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
    pub(crate) fn spans_of(&self, unit: usize, maps: &[SourceMap]) -> BTreeMap<String, usize> {
        let mut out: BTreeMap<String, usize> = BTreeMap::new();
        for map in maps {
            for marker in &map.markers {
                // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
                if !own_marker(self.workspace(), &self.plan, unit, marker) {
                    continue;
                }
                // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
                let lines = marker.end.saturating_sub(marker.output_line) + 1;
                *out.entry(region_named(marker)).or_default() += lines;
            }
        }
        out
    }

    /// The problems of every rejected unit: one rejected line per unit, then its problems
    /// indented below it.
    // @lfy def/cli/main.lfy:main#main:main:edc0f227f5209e85d4be95c8b95210cd16f09cf786498eb2bc58d9ea7f7ea385
    pub(crate) fn report_rejections(&mut self, batch: &Batch, verdicts: &[Verdict]) -> usize {
        let mut rejected = 0;
        for (&unit, verdict) in batch.units.iter().zip(verdicts) {
            if verdict.accepted {
                continue;
            }
            rejected += 1;
            let stem = self.plan.units[unit].stem.clone();
            // @lfy def/cli/main.lfy:main#main:main:edc0f227f5209e85d4be95c8b95210cd16f09cf786498eb2bc58d9ea7f7ea385
            let message = verdict.problems.join("\n");
            self.reporter.report(Step::Rejected, Some(&batch.identifier), Some(&stem), &message);
            // @lfy def/cli/main.lfy:main#main:main:edc0f227f5209e85d4be95c8b95210cd16f09cf786498eb2bc58d9ea7f7ea385
            self.reporter.problems(&verdict.problems);
        }
        rejected
    }

    /// A batch whose units were accepted and written back, and whose reviews then violated
    /// what was asked: the acceptance was structural, and the review retracts it, so the
    /// finished line and the code count the units as rejected rather than accepted. Its
    /// held source maps are dropped rather than recorded, so its outputs stay on disk as
    /// they are and the unit stays planned.
    // @lfy def/cli/main.lfy:main#main:main:e9c86cec438cdb4badb2de33afdcb601c2b149059793a66717645f2b181e4894
    pub(crate) fn unrecord(&mut self, batch: &Batch) {
        for &unit in &batch.units {
            if self.recorded.remove(&self.plan.units[unit].stem) {
                self.accepted = self.accepted.saturating_sub(1);
                self.rejected += 1;
            }
        }
        self.reporter.done = self.accepted;
    }
}
