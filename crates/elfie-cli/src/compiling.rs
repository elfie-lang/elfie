// @lfy def/cli/main.lfy:main
//! The compile command: the map files it tidies before any batch runs, the plan it prints
//! with --dry-run, and one batch compiled from its first run to its outcome.

use super::*;
use crate::copies::{KEPT, Kept, Where};
use crate::processes::Runs;
use crate::running::Run;
use crate::schedule::{Round, changes_of, round_of, run_batches};

/// How many batches a compile runs at once: what --jobs says where a batch can have its own
/// copy of the root, and one where it cannot.
///
/// Outside a git repository nothing says what belongs to the project, so no batch gets a
/// copy and every one of them would run in the root itself, breaking each other's builds and
/// tests. They run one at a time there, in plan order, however --jobs is given.
// @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
pub(crate) fn compile_jobs(invocation: &Invocation, root: &Path) -> usize {
    // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
    match git_directory(root) {
        // @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
        Some(_) => jobs_of(invocation),
        // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
        None => 1,
    }
}

/// What earlier versions recorded every map of a target in, under its output directory.
// @lfy def/cli/main.lfy:main
pub(crate) const LEGACY_MAPS: &str = "source-map.json";

/// The output directory of each planned target whose maps were read from source-map.json
/// rather than from a folder of map files, so that a compile moves them before any batch
/// runs.
///
/// A target whose folder of map files is there had its maps read from that folder, which is
/// where every map recorded since goes, so its source-map.json is not what the plan was made
/// from and nothing is moved out of it.
// @lfy def/cli/main.lfy:main#main:main:67eaaa8a1841a575fb7441eb3ad67ea695d923595769380ebe74e710e6251abf
pub(crate) fn legacy_targets(workspace: &Workspace, target: Option<&str>) -> Vec<String> {
    let mut found = Vec::new();
    for named in &workspace.targets {
        if target.is_some_and(|target| named.identifier != target) {
            continue;
        }
        // @lfy def/cli/main.lfy:main#main:main:67eaaa8a1841a575fb7441eb3ad67ea695d923595769380ebe74e710e6251abf
        if workspace.root.join(MAPS).join(&named.identifier).is_dir() {
            continue;
        }
        if workspace.root.join(&named.output_directory).join(LEGACY_MAPS).is_file() {
            found.push(named.output_directory.clone());
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The maps a planned target kept in source-map.json recorded in each unit's map file, and
/// the file removed once every one is recorded; a unit whose maps could not be recorded
/// leaves the file in place rather than removed, so the maps it holds are not lost.
// @lfy def/cli/main.lfy:main#main:main:67eaaa8a1841a575fb7441eb3ad67ea695d923595769380ebe74e710e6251abf
// @lfy def/cli/main.lfy:main#main:main:b9ad771de6ec1ee6e2812d5335aef919fe9cd8228cb6b2fea9731de46c6dd24e
pub(crate) fn move_legacy_maps(workspace: &Workspace, plan: &Plan, directories: &[String]) {
    for directory in directories {
        let path = workspace.root.join(directory).join(LEGACY_MAPS);
        let held = generation::read_source_maps(&path);
        let mut every = true;
        for unit in &plan.units {
            let target = &workspace.targets[unit.target].identifier;
            let source = &workspace.files[unit.file].path;
            // @lfy def/cli/main.lfy:main
            let mine: Vec<SourceMap> =
                held.iter().filter(|map| &map.target == target && &map.source == source).cloned().collect();
            if mine.is_empty() {
                continue;
            }
            // @lfy def/cli/main.lfy:main#main:main:67eaaa8a1841a575fb7441eb3ad67ea695d923595769380ebe74e710e6251abf
            every &= generation::record(workspace, unit, &mine);
        }
        // Every one recorded, the file is removed; one that could not be leaves it in place
        // rather than removed, so the maps it holds are not lost.
        // @lfy def/cli/main.lfy:main#main:main:b9ad771de6ec1ee6e2812d5335aef919fe9cd8228cb6b2fea9731de46c6dd24e
        if every {
            // @lfy def/cli/main.lfy:main#main:main:08ac623b539225786793a4c37eb60950b68b85acbd4d4b5306bc4fc89fd8b9fb
            let _ = fs::remove_file(&path);
        }
    }
}

/// Every map file under the folder of a planned target that belongs to no unit of the plan,
/// because its definition file was deleted, moved, or no longer builds for the target,
/// removed before any batch runs.
// @lfy def/cli/main.lfy:main#main:main:531885eb3c4021855f9d9fbc7e806cedbc5da481cdcee0ca5e70c32b6edaad0d
pub(crate) fn remove_orphan_maps(workspace: &Workspace, plan: &Plan, target: Option<&str>) {
    let mine: BTreeSet<PathBuf> =
        plan.units.iter().map(|unit| generation::map_file(workspace, unit)).collect();
    for named in &workspace.targets {
        if target.is_some_and(|target| named.identifier != target) {
            continue;
        }
        let folder = workspace.root.join(MAPS).join(&named.identifier);
        let mut found = Vec::new();
        walk_all(&folder, &mut found);
        for path in found {
            // @lfy def/cli/main.lfy:main#main:main:531885eb3c4021855f9d9fbc7e806cedbc5da481cdcee0ca5e70c32b6edaad0d
            if path.extension().is_some_and(|e| e == "json") && !mine.contains(&path) {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

/// Every file under `elfie-compile/cache/kept` for a planned target that names a stem that is
/// no unit of the plan, removed before any batch runs: the work it kept answers for a unit
/// that is gone, so nothing may take it.
// @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
pub(crate) fn remove_orphan_kept(workspace: &Workspace, plan: &Plan, target: Option<&str>) {
    // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
    let mine: BTreeSet<&str> = plan.units.iter().map(|unit| unit.stem.as_str()).collect();
    // The targets the plan holds a unit for, which are the planned ones: work kept for any
    // other target is not this compile's to remove.
    // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
    let planned_targets: BTreeSet<&str> = plan
        .units
        .iter()
        .map(|unit| workspace.targets[unit.target].identifier.as_str())
        .filter(|named| target.is_none_or(|target| *named == target))
        .collect();
    let mut found = Vec::new();
    walk_all(&workspace.root.join(KEPT), &mut found);
    for path in found {
        if !path.extension().is_some_and(|extension| extension == "json") {
            continue;
        }
        // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
        let held = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| Kept::of_json(&value));
        let Some(held) = held else { continue };
        // Only the kept work of a planned target is looked at here; another target's is for
        // the compile that plans it.
        // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
        if !planned_targets.contains(held.target.as_str()) {
            continue;
        }
        // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
        if held.stems().iter().any(|stem| !mine.contains(stem)) {
            // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
            let _ = fs::remove_file(&path);
        }
    }
}

/// Where the review records of every unit of a target are kept, under the root.
// @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
pub(crate) const REVIEWS: &str = "elfie-compile/reviews";

/// Every review record file under the folder of a planned target that belongs to no unit of
/// the plan, because its definition file was deleted, moved, or no longer builds for the
/// target, removed before any batch runs.
// @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
pub(crate) fn remove_orphan_reviews(workspace: &Workspace, plan: &Plan, target: Option<&str>) {
    let mine: BTreeSet<PathBuf> =
        plan.units.iter().map(|unit| generation::review_file(workspace, unit)).collect();
    for named in &workspace.targets {
        if target.is_some_and(|target| named.identifier != target) {
            continue;
        }
        let folder = workspace.root.join(REVIEWS).join(&named.identifier);
        let mut found = Vec::new();
        walk_all(&folder, &mut found);
        for path in found {
            // @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
            if path.extension().is_some_and(|e| e == "json") && !mine.contains(&path) {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

/// One unit as `elfie_units` lists it: the target, the stem, the reason or up to date,
/// and the stems of its dependencies.
///
/// Without --json the target is muted, the stem is subject, the reason is painted in its own
/// tone, and the dependencies are muted; the words and their order stay as they are, so a
/// script splitting a line on spaces reads the same fields.
// @lfy def/cli/main.lfy:main#main:main:5f8a307089850766f7edc63e08cf04c35a137c49fe5052d7ac979ef3f1e491ea
pub(crate) fn unit_line(workspace: &Workspace, plan: &Plan, index: usize, on: bool) -> String {
    let unit = &plan.units[index];
    let reason = unit.reason.map_or_else(|| "up to date".to_string(), |r| r.as_str().to_string());
    let dependencies: Vec<&str> = unit.dependencies.iter().map(|&d| plan.units[d].stem.as_str()).collect();
    // @lfy def/cli/main.lfy:main#main:main:84bb3a6e0235d5d36cc865a418d3f617e6b697d65fd85324d85cc995e44a3e89
    format!(
        "{} {} {} {}",
        paint(&workspace.targets[unit.target].identifier, Tone::Muted, on),
        paint(&unit.stem, Tone::Subject, on),
        paint(&reason, reason_tone(unit.reason), on),
        paint(&format!("[{}]", dependencies.join(", ")), Tone::Muted, on)
    )
}

/// One batch as `elfie_units` lists it: its identifier and the stems it holds. Without --json
/// the word batch is muted, the identifier is subject, and the stems are muted.
// @lfy def/cli/main.lfy:main
pub(crate) fn batch_line(plan: &Plan, batch: &Batch, on: bool) -> String {
    // @lfy def/cli/main.lfy:main#main:main:1a9fbb6611deaa4c81a556016aa08baa3d109ea425f6b509c01be7fe23a5ae32
    format!(
        "{} {} {}",
        paint("batch", Tone::Muted, on),
        paint(&batch.identifier, Tone::Subject, on),
        paint(&format!("[{}]", stems_of(plan, batch, ", ")), Tone::Muted, on)
    )
}

/// One of [`Plan::moves`] as a compile prints it: the word move, the target, the stem of the
/// origin, the word into, the stem of the unit that compiles them, the identifiers of the
/// entities moved in brackets, the word for, and the cause.
///
/// Painted, the target and the words move, into, and for are muted, the two stems are
/// subject, and the identifiers and the cause are plain; the words and their order stay as
/// they are either way.
// @lfy def/cli/main.lfy:main#main:main:0f5a7a1258a11b49d77120109972e47f61867f0f8fe2d59078aa9bfa4ffb1849
// @lfy def/cli/main.lfy:main#main:main:d76dbb95b25d73fb984ddecdb07366d73e2c932574c9a94208ad9df30c8d8933
pub(crate) fn move_text(workspace: &Workspace, plan: &Plan, moved: &Move, on: bool) -> String {
    let origin = &plan.units[moved.origin];
    // @lfy def/cli/main.lfy:main#main:main:d76dbb95b25d73fb984ddecdb07366d73e2c932574c9a94208ad9df30c8d8933
    format!(
        "{} {} {} {} {} [{}] {} {}",
        paint("move", Tone::Muted, on),
        paint(&workspace.targets[origin.target].identifier, Tone::Muted, on),
        paint(&origin.stem, Tone::Subject, on),
        paint("into", Tone::Muted, on),
        paint(&plan.units[moved.into].stem, Tone::Subject, on),
        moved_entities(workspace, moved),
        paint("for", Tone::Muted, on),
        moved.cause
    )
}

/// Each unit is printed as `elfie_units` lists it, then each batch with the stems it
/// holds, and nothing is generated.
// @lfy def/cli/main.lfy:main#main:main:5f8a307089850766f7edc63e08cf04c35a137c49fe5052d7ac979ef3f1e491ea
pub(crate) fn dry_run(workspace: &Workspace, plan: &Plan, target: Option<&str>, json: bool, on: bool) -> ExitCode {
    // Nothing is generated: the move lines come first, then the unit lines, then the batch
    // lines.
    // @lfy def/cli/main.lfy:main#main:main:2e8045c1f86a83462e1d79b1bdf98da4979fc1d0edf4301ad3cf8b989dc12004
    // @lfy def/cli/main.lfy:main#main:main:96490b9cde09b44c6fecc26b1a57225e5a6f9c756adeff05a82c394cfcf6e886
    // Before the units, each move that resolved a cycle among files.
    // @lfy def/cli/main.lfy:main#main:main:0f5a7a1258a11b49d77120109972e47f61867f0f8fe2d59078aa9bfa4ffb1849
    for moved in &plan.moves {
        let origin = &plan.units[moved.origin];
        if target.is_some_and(|name| workspace.targets[origin.target].identifier != name) {
            continue;
        }
        if json {
            // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
            println!(
                "{}",
                serde_json::json!({
                    "move": origin.stem,
                    "target": workspace.targets[origin.target].identifier,
                    "into": plan.units[moved.into].stem,
                    "entities": moved
                        .entities
                        .iter()
                        .map(|&entity| workspace.model.entities[entity].identifier.clone())
                        .collect::<Vec<_>>(),
                    "cause": moved.cause,
                })
            );
        } else {
            // @lfy def/cli/main.lfy:main#main:main:d76dbb95b25d73fb984ddecdb07366d73e2c932574c9a94208ad9df30c8d8933
            println!("{}", move_text(workspace, plan, moved, on));
        }
    }
    for index in 0..plan.units.len() {
        let unit = &plan.units[index];
        if target.is_some_and(|name| workspace.targets[unit.target].identifier != name) {
            continue;
        }
        if json {
            // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
            println!(
                "{}",
                serde_json::json!({
                    "target": workspace.targets[unit.target].identifier,
                    "stem": unit.stem,
                    "file": workspace.files[unit.file].path,
                    "reason": unit.reason.map(|r| r.as_str()),
                    "dependencies": unit.dependencies.iter().map(|&d| plan.units[d].stem.clone()).collect::<Vec<_>>(),
                })
            );
        } else {
            println!("{}", unit_line(workspace, plan, index, on));
        }
    }
    // Then each batch, on a line reading the word batch, its identifier, and the stems it
    // holds.
    // @lfy def/cli/main.lfy:main#main:main:e5ba46ad9fe0f231f9b65946a77625a3a1049f4496f157325f08ca062c200285
    for batch in &plan.batches {
        if target.is_some_and(|name| batch_target(workspace, plan, batch) != name) {
            continue;
        }
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "batch": batch.identifier,
                    "stems": batch.units.iter().map(|&unit| plan.units[unit].stem.clone()).collect::<Vec<_>>(),
                })
            );
        } else {
            println!("{}", batch_line(plan, batch, on));
        }
    }
    ExitCode::Success
}

/// The problems of an attempt appended to its instructions, so that the batch is run once
/// more knowing what was wrong.
// @lfy def/cli/main.lfy:main#main:main:17f6feee3279ee2519b458a9ca13607a4738441827bb862fc9a71d712ea28691
// @lfy def/cli/main.lfy:main#main:main:b136b63971e67fd1f1b4e0b4a407ebe829061ebda953131f2a62dc4d2993330f
pub(crate) fn append_problems(request: &mut Request, problems: &[String]) {
    request.instructions.push_str("\n\n## Problems with the previous attempt\n\n");
    for problem in problems {
        request.instructions.push_str(&format!("- {problem}\n"));
    }
}

impl Run {
    /// Runs the compiler on one batch, at most twice, and acts on the outcome; a batch
    /// whose reviews violate what was asked is run once more beyond that. Gives the source
    /// maps its units earned when it was accepted, to be recorded once its work has reached
    /// the root, and nothing when it stopped.
    ///
    /// A batch whose every unit is reworded only, or whose every unit has the reason
    /// dependency, may have its first run skip the compiler: the markers of its reworded
    /// criteria and tests are moved, or the target's build and test commands run, and its
    /// outputs are then checked as after a run of the compiler. A skipped run counts as no
    /// run of the compiler in any count that decides whether the batch is run once more or
    /// stops, and anything that goes wrong sends it to the compiler as its first run.
    // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
    // @lfy def/cli/main.lfy:main#main:main:93bc38c041972030b9d2fabf83040193223e16bc9dc0d57762b9fdcea1eb2753
    // @lfy def/cli/main.lfy:main#main:main:97bd4222037bff6cb9965f2bb80eda7bdf1510086acdf2043dba6c4f6118713f
    pub(crate) fn compile_batch(
        run: &Mutex<Run>,
        batch: &Batch,
        command: &str,
        where_: &Where,
        runs: &Runs,
        kept: Option<&Kept>,
        round: &Round,
    ) -> Compiled {
        let directory = where_.directory().to_path_buf();
        // What may stand in for the compiler on the batch's first run, decided before any
        // marker of its outputs is rewritten: the outputs on disk must be exactly the ones
        // the last acceptance recorded, and nothing of a rejected run may have been taken.
        // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
        // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
        // @lfy def/cli/main.lfy:main#main:main:c8973e9e2d07a79f390133eefde25b3157ae35c014f978eb58e3a63d4f7eada0
        let stand = kept.is_none() && locked(run).outputs_stand(batch, &directory);
        let mut request = {
            let mut state = locked(run);
            let stems = stems_of(&state.plan, batch, " ");
            state.reporter.report(Step::Requesting, Some(&batch.identifier), None, &stems);
            // Before each request, the markers of each unit's reworded criteria and tests
            // name the ids they have now.
            // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
            state.reword_outputs(batch, &directory);
            // The compiler of a batch runs in a hosted session of this compile's agent
            // server, so its request names its interfaces and the tools that session offers.
            // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
            state.request_of(batch, &directory, true, kept)
        };
        // @lfy def/cli/main.lfy:main#main:main:97bd4222037bff6cb9965f2bb80eda7bdf1510086acdf2043dba6c4f6118713f
        let mut stage = locked(run).stage_of(batch, &request, stand);
        let mut attempt = 0;
        // The batch is run once more for a rejected outcome, and once more for violated
        // reviews even when it was already run once more for a rejected outcome.
        // @lfy def/cli/main.lfy:main#main:main:17f6feee3279ee2519b458a9ca13607a4738441827bb862fc9a71d712ea28691
        let mut retried_for_rejection = false;
        let mut retried_for_reviews = false;
        let mut retried_for_failure = false;
        // Whether the next run of the compiler, and the next run of the verifier, go on from
        // the session of that role's run on the batch here before it.
        // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
        let mut resuming_compiler = false;
        let mut resuming_verifier = false;
        // What the batch's first review carries: what each of its units was last reviewed as.
        // @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
        let mut records = locked(run).records_of(batch);
        loop {
            // The compile stopped while the batch was running: nothing more starts for it,
            // and nothing of it is merged into the root or recorded.
            // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
            if locked(run).halts(&batch.identifier) {
                // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
                return Compiled::stopped();
            }
            // A first run that skips the compiler: the target's build and test commands, or
            // nothing but the markers already moved, and then the outputs checked.
            // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
            // @lfy def/cli/main.lfy:main#main:main:c9e9379aee6642c4c6fd644592fb507b9275e32d2a6daa8928f697b3e30a5b06
            let outcome = if stage == Stage::Compiler {
                attempt += 1;
                let (units, binding) = {
                    let mut state = locked(run);
                    let step = if attempt == 1 { Step::Compiling } else { Step::Retrying };
                    state.reporter.report(step, Some(&batch.identifier), None, command);
                    // The run is run as the compiler of this batch, in the batch's directory,
                    // with the request its input is made from, no review, and no source maps:
                    // a compiler is given no verdict to read.
                    // @lfy def/cli/main.lfy:main#main:main:49b8e07b5de76a70728002b1123896a9bea926e67c9452ff6c2c821e84281a04
                    let binding = state.binding_of(
                        Role::Compiler,
                        Some(batch),
                        &directory,
                        Some(request.clone()),
                        None,
                        Vec::new(),
                    );
                    // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
                    (stems_of(&state.plan, batch, " "), binding)
                };
                // The lock is free here, so another batch streams its own compiler meanwhile.
                // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
                let exit = runs.run(
                    command,
                    where_,
                    &batch.identifier,
                    &units,
                    &request.instructions,
                    binding,
                    resuming_compiler,
                );
                // The compile stopped while the run was going, so the run was stopped with
                // every process of its group: what it gives is ignored, and nothing of the
                // batch is merged into the root or recorded.
                // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
                if locked(run).halts(&batch.identifier) {
                    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
                    return Compiled::stopped();
                }
                let mut state = locked(run);
                // The code is -1 because the command could not be started: the outcome is
                // failed.
                // @lfy def/cli/main.lfy:main#main:main:0dff09f6c30f73ee8dcc8aa60016fb5af42b0b31adeaa63cec1108324df750ae
                if exit.code == -1 {
                    Outcome {
                        kind: OutcomeKind::Failed,
                        message: format!("the compiler command could not be run: {}", exit.stderr.trim()),
                        verdicts: Vec::new(),
                    }
                } else {
                    // Process.Exit.stdout is the report, appended to the log.
                    // @lfy def/cli/main.lfy:main#main:main:f507a20d5658ce7b158a66e802e963debf7d67da97fd22a6992b840100d837b2
                    // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
                    let report = exit.stdout;
                    state
                        .reporter
                        .append(&format!("--- the report of {} (attempt {attempt}) ---\n{report}", batch.identifier));
                    // @lfy def/cli/main.lfy:main#main:main:d32bb5f8cb99bd6f9ba335bb675a075d075910a07c8e5639cd4c008ec9bfea4b
                    let verdicts = state.verdicts_of(batch, &request, &directory);
                    // How the run ended is the outcome of its report, those verdicts, and
                    // what it submitted through its session before the session was closed.
                    // @lfy def/cli/main.lfy:main#main:main:37a3061e2d1da4becaf9fd160790f16ed84d77d4efa8b867622b0546149e5262
                    generation::outcome_of(&report, verdicts, exit.record.submission.as_ref())
                }
            } else {
                // The target's build command and then its test command stand in for the
                // compiler on a batch whose every unit has the reason dependency; a command
                // that failed sends the batch to the compiler as its first run.
                // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
                // @lfy def/cli/main.lfy:main#main:main:9253f376492f06a33bd0110d5f3f29c04bffd6a3b5a3a922f56cb3f2d9a668e9
                if stage == Stage::Dependency
                    && let Err(why) = Run::build_and_test(run, batch, &directory, runs)
                {
                    // @lfy def/cli/main.lfy:main#main:main:9253f376492f06a33bd0110d5f3f29c04bffd6a3b5a3a922f56cb3f2d9a668e9
                    // @lfy def/cli/main.lfy:main#main:main:93bc38c041972030b9d2fabf83040193223e16bc9dc0d57762b9fdcea1eb2753
                    locked(run).reporter.report(Step::Building, Some(&batch.identifier), None, &why);
                    stage = Stage::Compiler;
                    continue;
                }
                // Each unit is accepted or rejected as after a run of the compiler.
                // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
                // @lfy def/cli/main.lfy:main#main:main:c9e9379aee6642c4c6fd644592fb507b9275e32d2a6daa8928f697b3e30a5b06
                let verdicts = locked(run).verdicts_of(batch, &request, &directory);
                // A unit rejected without a run of the compiler: the problems are printed and
                // the compiler runs on the batch as its first run, counting toward no retry.
                // @lfy def/cli/main.lfy:main#main:main:b8f2ed1f4f7dd347c1fc2031d346ef69e5e80252bfc60209de5f60e9d30ea618
                // @lfy def/cli/main.lfy:main#main:main:9253f376492f06a33bd0110d5f3f29c04bffd6a3b5a3a922f56cb3f2d9a668e9
                if verdicts.iter().any(|verdict| !verdict.accepted) {
                    let mut state = locked(run);
                    // @lfy def/cli/main.lfy:main#main:main:b8f2ed1f4f7dd347c1fc2031d346ef69e5e80252bfc60209de5f60e9d30ea618
                    state.report_rejections(batch, &verdicts);
                    if stage == Stage::Dependency {
                        // The line names each unit that was rejected.
                        // @lfy def/cli/main.lfy:main#main:main:9253f376492f06a33bd0110d5f3f29c04bffd6a3b5a3a922f56cb3f2d9a668e9
                        let rejected: Vec<&str> = batch
                            .units
                            .iter()
                            .zip(&verdicts)
                            .filter(|(_, verdict)| !verdict.accepted)
                            .map(|(&unit, _)| state.plan.units[unit].stem.as_str())
                            .collect();
                        let why = format!("{} was rejected after the build and the test passed", rejected.join(", "));
                        state.reporter.report(Step::Building, Some(&batch.identifier), None, &why);
                    }
                    drop(state);
                    // @lfy def/cli/main.lfy:main#main:main:93bc38c041972030b9d2fabf83040193223e16bc9dc0d57762b9fdcea1eb2753
                    stage = Stage::Compiler;
                    continue;
                }
                // Every unit was accepted: a batch accepted on its build and test writes its
                // outputs back and is done there, with no verifier run on it; a batch accepted
                // on its moved markers alone goes on as a compiler run whose outcome is
                // accepted does, which writes them back once.
                // @lfy def/cli/main.lfy:main#main:main:b82042daa1550092c2eeae599eba3e1056e1c9987779953b6b11500318102df7
                if stage == Stage::Dependency {
                    // @lfy def/cli/main.lfy:main#main:main:b82042daa1550092c2eeae599eba3e1056e1c9987779953b6b11500318102df7
                    let held = locked(run).hold_batch(batch, &verdicts, &directory);
                    // Every unit was accepted and nothing is to run the compiler on the
                    // batch: what it has accepted is what a copy depending on it takes.
                    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
                    round.settled(changes_of(where_));
                    // @lfy def/cli/main.lfy:main#main:main:b82042daa1550092c2eeae599eba3e1056e1c9987779953b6b11500318102df7
                    return Compiled::on_build(held);
                }
                // @lfy def/cli/main.lfy:main#main:main:64edfa7c41c1dd3ba97140f316748c707c8d82d2e2e9691e156ae3ba2749260f
                Outcome { kind: OutcomeKind::Accepted, message: String::new(), verdicts }
            };
            match outcome.kind {
                // Each unit's outputs are written back in the batch's directory and its
                // source maps are held, and the batch is verified; no review violated, or
                // nothing verifying it, leaves the held source maps to be recorded once the
                // batch's work has reached the root.
                // @lfy def/cli/main.lfy:main#main:main:c0ff42fd47c552fedc4e3213d3ec87968ae59279faec3d132caf6866820027d7
                OutcomeKind::Accepted => {
                    // Each unit's outputs are written back once, whether the compiler ran or
                    // the batch's moved markers stood in for it.
                    // @lfy def/cli/main.lfy:main#main:main:c0ff42fd47c552fedc4e3213d3ec87968ae59279faec3d132caf6866820027d7
                    // @lfy def/cli/main.lfy:main#main:main:64edfa7c41c1dd3ba97140f316748c707c8d82d2e2e9691e156ae3ba2749260f
                    let held = locked(run).hold_batch(batch, &outcome.verdicts, &directory);
                    // Every unit of the batch was accepted and nothing is about to run the
                    // compiler on it again: what it has accepted no longer changes, so a copy
                    // of a batch depending on it may take it before its review ends.
                    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
                    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
                    round.settled(changes_of(where_));
                    // The batch is verified; no review violated, or nothing verifying it,
                    // and the batch is done.
                    // @lfy def/cli/main.lfy:main#main:main:79415ab35d3d0d869ed27ece2f7e00f6466563a94f8acf3d056c77ec329cd386
                    // @lfy def/cli/main.lfy:main#main:main:5841e3d28e75b69d42ed2ba411b34aa9429ca7415424978b7b95493fed283150
                    // @lfy def/cli/main.lfy:main#main:main:991ed2ddd9c68b7df2156a615cae29e3dc3add9bd395dfb4fffaebafeb35a4c2
                    let Some(verified) =
                        Run::verify_batch(run, batch, where_, &held, &records, runs, resuming_verifier)
                    else {
                        // A compile that stopped while the verifier ran stopped that run too:
                        // nothing of the batch is merged into the root or recorded.
                        // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
                        if locked(run).halts(&batch.identifier) {
                            // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
                            return Compiled::stopped();
                        }
                        return Compiled::accepted(held);
                    };
                    let problems = verified.problems;
                    // The review made once more carries what this one settled.
                    // @lfy def/cli/main.lfy:main#main:main:84bbcb2516b2cafb1bfbde0f772cadb0875451d4f6704a126825d45438d436dc
                    records = verified.next;
                    let mut state = locked(run);
                    // A violated review or an unplaced violation is handled as a rejected
                    // outcome is: the problems are printed and the batch is run once more
                    // with them appended to the instructions, even when it was already run
                    // once more for a rejected outcome. The verifier then runs on it again.
                    // @lfy def/cli/main.lfy:main#main:main:a90dd937c3211a26fb1a136a29a7052114ef2561c7d039de63fe390de2dec33e
                    // @lfy def/cli/main.lfy:main#main:main:84bbcb2516b2cafb1bfbde0f772cadb0875451d4f6704a126825d45438d436dc
                    state.reporter.report(Step::Rejected, Some(&batch.identifier), None, &problems.join("\n"));
                    state.reporter.problems(&problems);
                    // The second such report stops the batch as rejected, and the held source
                    // maps are dropped rather than recorded, so its outputs are left on disk
                    // and the unit stays planned. Its copy keeps the work for the next
                    // compile of the batch.
                    // @lfy def/cli/main.lfy:main#main:main:60e5c32018713d03196fbe6b68625288ef878c3b8e6a489007fc2126d39b9a6a
                    // @lfy def/cli/main.lfy:main#main:main:e9c86cec438cdb4badb2de33afdcb601c2b149059793a66717645f2b181e4894
                    // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
                    if retried_for_reviews {
                        state.unrecord(batch);
                        state.worsen(ExitCode::Problems);
                        state.stop(batch);
                        // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
                        return Compiled::rejected(problems);
                    }
                    retried_for_reviews = true;
                    // The batch is about to run the compiler again, so it is no longer far
                    // enough along for a batch depending on it to take what it accepted.
                    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
                    round.runs_again();
                    // The batch's run once more is the compiler's: its first run when the
                    // compiler has not run on it yet, and a resuming run when it has.
                    // @lfy def/cli/main.lfy:main#main:main:2db65eab2f47acc8e9484b0ddac5774576ff7e5ed00b482fb43f68869baa1c60
                    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
                    resuming_compiler = attempt > 0;
                    resuming_verifier = true;
                    // @lfy def/cli/main.lfy:main#main:main:2db65eab2f47acc8e9484b0ddac5774576ff7e5ed00b482fb43f68869baa1c60
                    stage = Stage::Compiler;
                    drop(state);
                    // @lfy def/cli/main.lfy:main#main:main:ccb77bf35fa2892badac90b1cd9147551248086a7c8b8e46447f25e714ed6198
                    append_problems(&mut request, &problems);
                }
                // The problems are printed, and the batch is run once more with them
                // appended to the instructions.
                // @lfy def/cli/main.lfy:main#main:main:17f6feee3279ee2519b458a9ca13607a4738441827bb862fc9a71d712ea28691
                OutcomeKind::Rejected => {
                    let mut state = locked(run);
                    let rejected = state.report_rejections(batch, &outcome.verdicts);
                    let problems: Vec<String> =
                        outcome.verdicts.iter().flat_map(|verdict| verdict.problems.iter().cloned()).collect();
                    // A second rejection stops the compile, and its copy keeps the work for
                    // the next compile of the batch.
                    // @lfy def/cli/main.lfy:main#main:main:b4d24a6285bcfe6faf8fa684cd04234bc339a6acb61cf4c5597af955903b8ba1
                    // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
                    if retried_for_rejection {
                        state.rejected += rejected;
                        state.worsen(ExitCode::Problems);
                        state.stop(batch);
                        // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
                        return Compiled::rejected(problems);
                    }
                    retried_for_rejection = true;
                    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
                    resuming_compiler = true;
                    drop(state);
                    append_problems(&mut request, &problems);
                }
                // The reason is printed and written, and the compile stops.
                // @lfy def/cli/main.lfy:main#main:main:b01f4990d763a4c86c9d8bc4995bc1c02eebe03f8a9f36d24f234cfd4e6042df
                OutcomeKind::Blocked => {
                    let mut state = locked(run);
                    state.blocked += 1;
                    state.reporter.report(Step::Blocked, Some(&batch.identifier), None, &outcome.message);
                    // The reason is printed.
                    // @lfy def/cli/main.lfy:main#main:main:7b4c327653decbf264373384e604a80eaf1dd98b7513acb659e6462dc8c7522a
                    state.reporter.reason(&outcome.message);
                    let note = format!("# The compiler is blocked on the batch {}\n\n{}\n", batch.identifier, outcome.message);
                    state.write_note(batch, "blocked.md", &note);
                    state.worsen(ExitCode::Problems);
                    // Without --continue the compile stops.
                    // @lfy def/cli/main.lfy:main#main:main:5c341e1e6a5ddabeb77300d7deadd7e5a86870008f1c24e99a5252f7b956a93f
                    state.stop(batch);
                    return Compiled::stopped();
                }
                // The question is printed and written, and the compile stops.
                // @lfy def/cli/main.lfy:main#main:main:0e04fca1d5a1398b0203e9dfa3d58b7324f1ef742ad13c350264b5b958456075
                OutcomeKind::Clarification => {
                    let mut state = locked(run);
                    state.reporter.report(Step::Clarification, Some(&batch.identifier), None, &outcome.message);
                    // The question is printed.
                    // @lfy def/cli/main.lfy:main#main:main:110f4900a8d312787d8bb7041198cb88f88e9ff546daa7bbbbd9255b40329ac1
                    state.reporter.reason(&outcome.message);
                    let note = format!(
                        "# The compiler asked about the batch {}\n\n{}\n\n{ANSWER_HEADING}\n\n<!-- Answer by editing the definitions, or write the answer below this line; the next compile of this batch appends it to the instructions. -->\n",
                        batch.identifier, outcome.message
                    );
                    state.write_note(batch, "question.md", &note);
                    state.worsen(ExitCode::Problems);
                    // Without --continue the compile stops.
                    // @lfy def/cli/main.lfy:main#main:main:6996d8037ef2ea7f994646839432d548664a2cbd82d484947f3530d9066f33bd
                    state.stop(batch);
                    return Compiled::stopped();
                }
                // The batch is run once more.
                // @lfy def/cli/main.lfy:main#main:main:0dff09f6c30f73ee8dcc8aa60016fb5af42b0b31adeaa63cec1108324df750ae
                OutcomeKind::Failed => {
                    let mut state = locked(run);
                    state.reporter.report(Step::Failed, Some(&batch.identifier), None, &outcome.message);
                    // @lfy def/cli/main.lfy:main#main:main:43f2aa5048680432b93b7802c6b6a48f5fcb659a985ce06c84d0358d44a2277d
                    state.reporter.problems(std::slice::from_ref(&outcome.message));
                    // A second failure stops the compile.
                    // @lfy def/cli/main.lfy:main#main:main:b4bf8bd2d74cf6932c3ed9afe7c76d862161f90df8e2ae4a6e2cb1728ba3e9b8
                    if retried_for_failure {
                        state.failed += 1;
                        state.worsen(ExitCode::Failure);
                        state.stop(batch);
                        return Compiled::stopped();
                    }
                    retried_for_failure = true;
                    // A run once more after a failed run, or after an exit code of -1, is no
                    // resuming run: there was nothing to go on from.
                    // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
                    resuming_compiler = false;
                }
            }
        }
    }

    /// What the batch's first run is: the compiler, unless something may stand in for it.
    ///
    /// A batch whose every unit is reworded only has the markers of those criteria and tests
    /// moved and its outputs checked, which asks no compiler, but only while a verifier may
    /// look at the result. A batch whose every unit has the reason dependency has the
    /// target's build and test commands run instead, while the target has a line for both.
    // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
    // @lfy def/cli/main.lfy:main#main:main:373a3000fa63b155721a53e3ee16c9fbd2ad88138c3fd6a1ec81ce3eeab80f12
    // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
    // @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
    // @lfy def/cli/main.lfy:main#main:main:97bd4222037bff6cb9965f2bb80eda7bdf1510086acdf2043dba6c4f6118713f
    // @lfy def/cli/main.lfy:main#main:main:c8973e9e2d07a79f390133eefde25b3157ae35c014f978eb58e3a63d4f7eada0
    pub(crate) fn stage_of(&self, batch: &Batch, request: &Request, stand: bool) -> Stage {
        // An output that is no longer what was accepted, or work a rejected run kept, leaves
        // the compiler to do.
        // @lfy def/cli/main.lfy:main#main:main:c8973e9e2d07a79f390133eefde25b3157ae35c014f978eb58e3a63d4f7eada0
        if !stand || self.accept_only {
            return Stage::Compiler;
        }
        // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
        let reworded_only = batch.units.iter().all(|&index| {
            let path = &self.workspace().files[self.plan.units[index].file].path;
            request.reworded_only.contains(path)
        });
        // Only a batch a verifier may look at is accepted on its moved markers alone.
        // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
        // @lfy def/cli/main.lfy:main#main:main:373a3000fa63b155721a53e3ee16c9fbd2ad88138c3fd6a1ec81ce3eeab80f12
        if reworded_only && self.verifier.is_some() && !self.no_verify {
            return Stage::Reworded;
        }
        // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
        let dependency_only = batch
            .units
            .iter()
            .all(|&index| self.plan.units[index].reason == Some(Reason::Dependency));
        // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
        // @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
        if dependency_only && self.builds_and_tests(batch) {
            return Stage::Dependency;
        }
        // @lfy def/cli/main.lfy:main#main:main:97bd4222037bff6cb9965f2bb80eda7bdf1510086acdf2043dba6c4f6118713f
        Stage::Compiler
    }

    /// No compiler is run; the outputs already on disk are checked, written back
    /// normalized, and recorded when accepted. A batch whose units were all accepted is
    /// verified like any other against the source maps held for it; since no compiler runs
    /// there is nothing to run again, so a violated review rejects the batch at once and
    /// those maps are never recorded.
    // @lfy def/cli/main.lfy:main#main:main:0f1a513e22a68d8f32e1ff580303b2671ce0e15b513648dfe24f469f1567ff27
    pub(crate) fn accept_on_disk(run: &Mutex<Run>, batch: &Batch, root: &Path) {
        // No compiler is run; nothing else is writing the root, so the batch needs no copy of
        // its own.
        // @lfy def/cli/main.lfy:main#main:main:c51341f34278d51ccda0b1a75908b211f46b7db736525ed29c88351db922c346
        // @lfy def/cli/main.lfy:main#main:main:0f1a513e22a68d8f32e1ff580303b2671ce0e15b513648dfe24f469f1567ff27
        let where_ = Where::Root(root.to_path_buf());
        let held = {
            let mut state = locked(run);
            // Before the outputs are checked, the markers of each unit's reworded criteria
            // and tests name the ids they have now, written at their paths in the root.
            // @lfy def/cli/main.lfy:main#main:main:561bd7cbbedaf15b591306a1bda08fd34f7da689d466b406d196bcf906444f7d
            state.reword_outputs(batch, root);
            // Nothing reads these instructions: with --accept no compiler is run and no
            // session is hosted, so the request is for no hosted session.
            // @lfy def/cli/main.lfy:main#main:main:c51341f34278d51ccda0b1a75908b211f46b7db736525ed29c88351db922c346
            // @lfy def/cli/main.lfy:main#main:main:8444ea8a2d9ff6b491b88ffdab12f7f13f9596b9dfd77f20d2a51a121c7cad69
            let request = state.request_of(batch, root, false, None);
            let verdicts = state.verdicts_of(batch, &request, root);
            let accepted: Vec<Verdict> = verdicts.iter().filter(|v| v.accepted).cloned().collect();
            let mut held = state.maps.clone();
            if !accepted.is_empty() {
                let only = Batch {
                    units: batch.units.iter().copied().zip(&verdicts).filter(|(_, v)| v.accepted).map(|(unit, _)| unit).collect(),
                    identifier: batch.identifier.clone(),
                };
                held = state.hold_batch(&only, &accepted, root);
            }
            let rejected = state.report_rejections(batch, &verdicts);
            // A rejected unit makes the code problems; what was accepted is recorded all the
            // same, so a compiler that worked through the agent server has its work recorded.
            // @lfy def/cli/main.lfy:main#main:main:52e13f2436af16c72ca604f2b05b1b8b9579d7f785a46580d36ca9a5d0c7bdc9
            // @lfy def/cli/main.lfy:main#main:main:906a7b942e9fdeeb2d0a33ef27f1952dfd4538adf43ab27446a9ee3d464d2e24
            if rejected > 0 {
                state.record_maps(batch, &held, false);
                state.rejected += rejected;
                state.worsen(ExitCode::Problems);
                return;
            }
            held
        };
        // @lfy def/cli/main.lfy:main#main:main:b136b63971e67fd1f1b4e0b4a407ebe829061ebda953131f2a62dc4d2993330f
        let records = locked(run).records_of(batch);
        // With --accept no compiler ran, so the verifier's run is a first run and goes on
        // from nothing.
        // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
        let runs = {
            let mut state = locked(run);
            let agent = state.agent.clone();
            Runs::new(agent, state.running_of(&batch.identifier))
        };
        let verified = Run::verify_batch(run, batch, &where_, &held, &records, &runs, false);
        let mut state = locked(run);
        if let Some(verified) = verified {
            let problems = verified.problems;
            state.reporter.report(Step::Rejected, Some(&batch.identifier), None, &problems.join("\n"));
            state.reporter.problems(&problems);
            state.unrecord(batch);
            state.worsen(ExitCode::Problems);
            state.stop(batch);
        } else {
            // @lfy def/cli/main.lfy:main#main:main:906a7b942e9fdeeb2d0a33ef27f1952dfd4538adf43ab27446a9ee3d464d2e24
            state.record_maps(batch, &held, false);
        }
    }

    /// The request of a batch written for a compiler run by hand, which is in no hosted
    /// session: its interfaces are quoted whole and it names none of a session's tools.
    // @lfy def/cli/main.lfy:main#main:main:de437a586cddefeefabed78d855e65916788f25c33c6c0d9b7286be02d199ce5
    pub(crate) fn write_request(&mut self, batch: &Batch) {
        let stems = stems_of(&self.plan, batch, " ");
        self.reporter.report(Step::Requesting, Some(&batch.identifier), None, &stems);
        let root = self.workspace().root.clone();
        // @lfy def/cli/main.lfy:main#main:main:de437a586cddefeefabed78d855e65916788f25c33c6c0d9b7286be02d199ce5
        let request = self.request_of(batch, &root, false, None);
        if let Some(path) = self.write_note(batch, "md", &request.instructions)
            && !self.json
        {
            println!("wrote {}", relative(&self.workspace().root, &path));
        }
    }

    /// The last line: the count accepted, rejected, blocked, and failed, each a tally, and
    /// every batch that did not complete on its own line, indented, painted warning. Its glyph
    /// and name are success when nothing was rejected, blocked, or failed and failure when
    /// something was.
    // @lfy def/cli/main.lfy:main#main:main:4da6fc0ef0b92bdd309365b39295a5cc5de6e9d3296556c8da4e7fc7e1c11920
    pub(crate) fn finish(&mut self) {
        // The count of units accepted and the word accepted, the count of units rejected and
        // the word rejected, the count of batches blocked and the word blocked, and the count
        // of batches failed and the word failed: each count and its own word alone, separated
        // by a space, and the four by a comma and a space.
        // @lfy def/cli/main.lfy:main#main:main:e86903be24031cd7d20e2b972c8d08d2fcbbc6ec9495571912401e5762cc2cd6
        // @lfy def/cli/main.lfy:main#main:main:081257eb508c08a673bc5c569b3099485967e6b85b434c98d303c2de2d9958c5
        let counts: Counts = vec![
            (self.accepted, "accepted", Tone::Success),
            (self.rejected, "rejected", Tone::Failure),
            (self.blocked, "blocked", Tone::Warning),
            (self.failed, "failed", Tone::Failure),
        ];
        // Nothing rejected, blocked, or failed: the glyph and name are success; something was:
        // they are failure.
        // @lfy def/cli/main.lfy:main#main:main:039d3bfd25c50ba9d34b15d77ecda97ff4ce12b3f3040faa8fadda1b972a2375
        // @lfy def/cli/main.lfy:main#main:main:8ad6cc67e5917e94bdf860ca278ad8a72e97989115f12275cd1d47dd6d7499d0
        let went_wrong = self.rejected > 0 || self.blocked > 0 || self.failed > 0;
        let tone = if went_wrong { Tone::Failure } else { Tone::Success };
        // With --json the one object's message is the four counts alone.
        // @lfy def/cli/main.lfy:main#main:main:bbad3c69582995a56ff9152a6a483894f2d276f7b0d0b4805da999767796b37e
        if self.json {
            let message = counts_message(&counts, false);
            self.reporter.report(Step::Finished, None, None, &message);
            return;
        }
        self.reporter.report_counts(Step::Finished, None, &counts, Some(tone));
        // The batches that did not complete follow on their own line, indented four spaces.
        // @lfy def/cli/main.lfy:main#main:main:df6beb5b55efabb4bf57333ff81f1f65c615c2988fc6090856921440a10c3639
        // @lfy def/cli/main.lfy:main#main:main:df6beb5b55efabb4bf57333ff81f1f65c615c2988fc6090856921440a10c3639
        if !self.incomplete.is_empty() {
            // The line is painted warning, as `reason` paints every line it prints.
            // @lfy def/cli/main.lfy:main#main:main:5e9f9e326f58c59bbc1092c42862138ad8fb59f86a69705b596007f531aa116c
            let line = format!("did not complete: {}", self.incomplete.join(", "));
            self.reporter.reason(&line);
        }
    }
}

/// What a batch's first run is: the compiler, or what may stand in for it.
// @lfy def/cli/main.lfy:main#main:main:97bd4222037bff6cb9965f2bb80eda7bdf1510086acdf2043dba6c4f6118713f
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// The compiler runs on the batch, as on any batch.
    // @lfy def/cli/main.lfy:main#main:main:97bd4222037bff6cb9965f2bb80eda7bdf1510086acdf2043dba6c4f6118713f
    Compiler,
    /// Every unit of the batch is reworded only, so moving the markers of its reworded
    /// criteria and tests is the whole of the work and its outputs are checked without the
    /// compiler.
    // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
    Reworded,
    /// Every unit of the batch has the reason dependency, so the target's build and test
    /// commands stand in for the compiler.
    // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
    Dependency,
}

/// What compiling one batch came to.
// @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
pub(crate) struct Compiled {
    /// The source maps its units earned, to be recorded once its work has reached the root;
    /// nothing when the batch stopped.
    // @lfy def/cli/main.lfy:main#main:main:5841e3d28e75b69d42ed2ba411b34aa9429ca7415424978b7b95493fed283150
    pub(crate) held: Option<Vec<SourceMap>>,
    /// Whether the batch was accepted on its build and test, so that no verifier ran on it
    /// and no review record file of its units changes.
    // @lfy def/cli/main.lfy:main#main:main:b82042daa1550092c2eeae599eba3e1056e1c9987779953b6b11500318102df7
    pub(crate) on_build: bool,
    /// The problems of the rejection that stopped it, in the order they were printed, so
    /// that a copy left behind keeps them with its work.
    // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
    pub(crate) problems: Vec<String>,
    /// Whether a second rejection stopped it, which is what makes a copy keep its work.
    // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
    pub(crate) keeps: bool,
}

impl Compiled {
    /// A batch whose units were accepted and reviewed, with the source maps they earned.
    // @lfy def/cli/main.lfy:main#main:main:5841e3d28e75b69d42ed2ba411b34aa9429ca7415424978b7b95493fed283150
    pub(crate) fn accepted(held: Vec<SourceMap>) -> Compiled {
        Compiled { held: Some(held), on_build: false, problems: Vec::new(), keeps: false }
    }

    /// A batch accepted on its build and test, which no verifier looked at.
    // @lfy def/cli/main.lfy:main#main:main:b82042daa1550092c2eeae599eba3e1056e1c9987779953b6b11500318102df7
    pub(crate) fn on_build(held: Vec<SourceMap>) -> Compiled {
        Compiled { held: Some(held), on_build: true, problems: Vec::new(), keeps: false }
    }

    /// A batch a second rejection stopped: its copy keeps the work and the problems.
    // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
    pub(crate) fn rejected(problems: Vec<String>) -> Compiled {
        Compiled { held: None, on_build: false, problems, keeps: true }
    }

    /// A batch that stopped for anything else: nothing of it is merged or recorded, and
    /// nothing of it is kept.
    // @lfy def/cli/main.lfy:main#main:main:8444ea8a2d9ff6b491b88ffdab12f7f13f9596b9dfd77f20d2a51a121c7cad69
    pub(crate) fn stopped() -> Compiled {
        Compiled { held: None, on_build: false, problems: Vec::new(), keeps: false }
    }
}

/// The diagnostics a compile prints when loading found an error: all of them, so a warning
/// stands beside the error that stopped the run. Empty when none of them is an error.
// @lfy def/cli/main.lfy:main#main:main:f2d15d5c350694370ffb5656be0859f0f69da753546ea57b01ed5e684923d727
pub(crate) fn loaded_diagnostics(workspace: &Workspace) -> Vec<Diagnostic> {
    let diagnostics = query::diagnostics_of(workspace, None);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        diagnostics
    } else {
        Vec::new()
    }
}

/// The diagnostics of the loaded program printed, when one of them is an error; whether one
/// was.
// @lfy def/cli/main.lfy:main#main:main:f2d15d5c350694370ffb5656be0859f0f69da753546ea57b01ed5e684923d727
pub(crate) fn print_loaded_diagnostics(workspace: &Workspace, json: bool, on: bool) -> bool {
    let diagnostics = loaded_diagnostics(workspace);
    for diagnostic in &diagnostics {
        print_diagnostic(diagnostic, json, on);
    }
    !diagnostics.is_empty()
}

/// Plans the units, runs the compiler on each batch in plan order, and records what it
/// produced.
// @lfy def/cli/main.lfy:main
pub(crate) fn compile(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    // @lfy def/cli/main.lfy:main#main:main:c0ff8dc13036c4a0c6308abadabef558ad51da34c43aeed7d60f6cc9a97a368c
    let workspace = workspace::load(root);
    let json = invocation.flag("json");
    let style = Paint::of(invocation);
    // The compiler is never handed a program with problems: no compiler runs, nothing is
    // generated, and the code is problems.
    // @lfy def/cli/main.lfy:main#main:main:f2d15d5c350694370ffb5656be0859f0f69da753546ea57b01ed5e684923d727
    // @lfy def/cli/main.lfy:main#main:main:3df84098154780d5074fdbf4d1209831b3e3f8bc87b7bc577035c1b63e2f443d
    if print_loaded_diagnostics(&workspace, json, style.out) {
        // @lfy def/cli/main.lfy:main#main:main:77ae4e6a5e2396d858d173399db2122dd9965d17bc44b4f05613dfae2362f448
        return ExitCode::Problems;
    }
    let target = match target_named(&workspace, invocation) {
        Ok(target) => target,
        Err(code) => return code,
    };
    // With --target the maps of that target alone are read, so no other target's map files
    // are opened.
    // @lfy def/cli/main.lfy:main#main:main:81eec0e83e4938bc3ddd792baad0cebc287a56344dcba0dfaf720cdfde05f3eb
    // @lfy def/cli/main.lfy:main#main:main:6b9feed8c0c1dd698132e42cc588b0234d6d80924e6e0ecb4b3e6d566b947e8d
    let maps = generation::source_maps_of(&workspace, target.as_deref());
    let legacy = legacy_targets(&workspace, target.as_deref());
    drop(workspace);
    // The units the last global review found violated.
    // @lfy def/cli/main.lfy:main#main:main:37aa77cbc42fd4dcc40b7262ac25164abbc8c899273439860ff39c8c39002a0d
    let (program, plan) = planned(root, &maps, &invocation.arguments, invocation.flag("all"), &violated_ids(root));
    // A cycle among files that no move resolves has no order to build in: its problem is
    // printed as an error and nothing is generated.
    // @lfy def/cli/main.lfy:main#main:main:055ed621ccb4cfc04dafc60c4ae42b146676fbac4139bd00ac26ddae87c1ad70
    for problem in &plan.problems {
        print_plan_problem(&program.workspace, problem, json, style.out);
    }
    let cycles = !plan.problems.is_empty();
    // @lfy def/cli/main.lfy:main#main:main:5f8a307089850766f7edc63e08cf04c35a137c49fe5052d7ac979ef3f1e491ea
    if invocation.flag("dry-run") {
        let code = dry_run(&program.workspace, &plan, target.as_deref(), json, style.out);
        // @lfy def/cli/main.lfy:main#main:main:055ed621ccb4cfc04dafc60c4ae42b146676fbac4139bd00ac26ddae87c1ad70
        return if cycles { ExitCode::Problems } else { code };
    }
    // Nothing is generated, and the code is problems.
    // @lfy def/cli/main.lfy:main#main:main:e0179e86c25f7960127d204e9f6499cb8cf0495adac3ba8441ff18238b46840d
    if cycles {
        // @lfy def/cli/main.lfy:main#main:main:d45b61bd01620de4f1c204c2a1a111fa2a8b3b2fff6d63bb3cf7faf41c8f0739
        return ExitCode::Problems;
    }
    // Before any batch runs, the maps a target kept in source-map.json are moved into the
    // units' map files and every map file belonging to no unit of the plan is removed.
    // @lfy def/cli/main.lfy:main#main:main:67eaaa8a1841a575fb7441eb3ad67ea695d923595769380ebe74e710e6251abf
    // @lfy def/cli/main.lfy:main#main:main:531885eb3c4021855f9d9fbc7e806cedbc5da481cdcee0ca5e70c32b6edaad0d
    move_legacy_maps(&program.workspace, &plan, &legacy);
    remove_orphan_maps(&program.workspace, &plan, target.as_deref());
    // The work kept for a batch that named a stem the plan no longer holds goes the same way,
    // so nothing takes work answering for a unit that is gone.
    // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
    remove_orphan_kept(&program.workspace, &plan, target.as_deref());
    // A review record file belonging to no unit of the plan is removed before any batch runs,
    // the same way, so that nothing carries a verdict for a unit that is gone.
    // With --accept no review record file changes, so none is removed either.
    // @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
    // @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
    if verifier_command(root).is_some() && !invocation.flag("no-verify") && !invocation.flag("accept") {
        remove_orphan_reviews(&program.workspace, &plan, target.as_deref());
    }

    let reporter = Reporter::new(json, style, 0, log_paths(root));
    let run = Mutex::new(Run::new(program, plan, maps.clone(), reporter, invocation));
    let compiler = compiler_command(root);
    let accept_only = invocation.flag("accept");
    // @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
    // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
    let jobs = compile_jobs(invocation, root);
    // The units a violated global review plans are compiled once more in the same compile,
    // and the global review runs once more after them.
    // @lfy def/cli/main.lfy:main#main:main:37688db5376890a46f433004a7e76905a0292b3adf375e72df9a7b7ebc9ef5f5
    let mut round = 1;
    loop {
        let round_of_batches = {
            let mut state = locked(&run);
            let batches = state.batches_for(target.as_deref());
            let total: usize = batches.iter().map(|&index| state.plan.batches[index].units.len()).sum();
            state.reporter.total = total;
            // The first progress line of a compile is planned, with the count of units
            // planned and of batches, whether anything was planned or not, so that a compile
            // with nothing to do still reads as one run.
            // @lfy def/cli/main.lfy:main#main:main:4da6fc0ef0b92bdd309365b39295a5cc5de6e9d3296556c8da4e7fc7e1c11920
            // @lfy def/cli/main.lfy:main#main:main:3b7ff1cb8c57ebc94c72273c9e46802fa9a2881815174d5e72e44b93e5ad00ea
            // The message counts the units planned, the batches, and the moves.
            // @lfy def/cli/main.lfy:main#main:main:2d74846758ea9c2aee36d43921093dbef40242c72894348bd117518c4fdcbe46
            let planned =
                format!("{total} units planned, {} batches, {} moves", batches.len(), state.plan.moves.len());
            state.reporter.report(Step::Planned, None, None, &planned);
            // The move lines follow the planned line, each indented four spaces.
            // @lfy def/cli/main.lfy:main#main:main:99a4adc185fa87df9a6aacaa34f84261addbd0756d5dc199405c5bc004f3e357
            if !json {
                for moved in &state.plan.moves {
                    let line = move_text(&state.program.workspace, &state.plan, moved, style.out);
                    println!("{INDENT}{line}");
                    let plain = move_text(&state.program.workspace, &state.plan, moved, false);
                    state.reporter.append(&format!("{INDENT}{plain}"));
                }
            }
            // @lfy def/cli/main.lfy:main#main:main:5b05d045fbdda6a51d4ac309b97ddac4c12d0a0e967117d6973df4878cc1bc96
            if total == 0 && round == 1 && !json {
                // @lfy def/cli/main.lfy:main#main:main:5b05d045fbdda6a51d4ac309b97ddac4c12d0a0e967117d6973df4878cc1bc96
                println!("{}", paint("every unit is up to date", Tone::Success, style.out));
            }
            state.accepted_batch = false;
            // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
            round_of(&state.plan, &batches)
        };
        // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
        run_batches(&run, &round_of_batches, root, compiler.as_deref(), accept_only, jobs);
        // Every batch has been handled and merged into the root: the global criteria and
        // tests are reviewed once, there.
        // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
        // @lfy def/cli/main.lfy:main#main:main:b560d79fb66587371f4a98bc89eea6db2f1123b1c6455f3310c1a9827b77b43e
        let violated = Run::global_review(&run, root);
        // The second global review of a compile stops it there, and the next compile plans
        // those units with reason violated.
        // @lfy def/cli/main.lfy:main#main:main:ffc6f99569b9c0ce1130f02761a91460937988ff1af7dea237a2802d41f23926
        if violated.is_empty() || round == 2 || accept_only || compiler.is_none() {
            // The second global review of a compile stops it there with the code problems.
            // @lfy def/cli/main.lfy:main#main:main:96dd6a88ebe0003d78a67421241c801e1f0e0faf8ced5a63a7825b67a31a2729
            if !violated.is_empty() {
                locked(&run).worsen(ExitCode::Problems);
            }
            break;
        }
        // The units it planned are compiled once more in the same compile, and the global
        // review runs once more after them.
        // @lfy def/cli/main.lfy:main#main:main:37688db5376890a46f433004a7e76905a0292b3adf375e72df9a7b7ebc9ef5f5
        // @lfy def/cli/main.lfy:main#main:main:5e145e7cf95605462ea683b7b1dffa98f7e1ee252c92165b2c3cda98cc745aea
        let (program, plan) = plan_with(root, &maps, &[], &violated);
        locked(&run).replan(program, plan, 0);
        round = 2;
    }
    // Every session of this compile is ended and the server stopped, after the last run has
    // returned and before the command returns, whatever the outcome.
    // @lfy def/cli/main.lfy:main#main:main:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
    let server = locked(&run).agent.server.clone();
    server.stop();
    let mut state = locked(&run);
    // The last progress line of a compile is finished, with the count accepted, rejected,
    // blocked, and failed.
    // @lfy def/cli/main.lfy:main#main:main:4da6fc0ef0b92bdd309365b39295a5cc5de6e9d3296556c8da4e7fc7e1c11920
    state.finish();
    // Every batch accepted is success; a rejection, a block, or a question is problems; a
    // command that could not run is failure.
    // @lfy def/cli/main.lfy:main#main:main:ebb4c47d5bade2bf74c3b8f602d49839293626ec296c19748df60b3f7ed90985
    // @lfy def/cli/main.lfy:main#main:main:6c350b9b103d6929538d70b4ac5e18843079843cb444d71311ec205b548c08f4
    // @lfy def/cli/main.lfy:main#main:main:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
    state.code
}
