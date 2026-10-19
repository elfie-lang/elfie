// @lfy def/cli/compile.lfy:compile
//! The compile command: the change set of each target it makes or resumes, the map files it
//! tidies before any unit runs, the plan it prints with --dry-run, the final gate each change
//! set passes, the global review, and the merge that hands the root every change set at once.

use super::*;
use crate::copies::{Changes, changes_of, copy_of, make_copy, merge_all, remove_change_set, sync_copy, write_merges};
use crate::processes::{line_for, run_target_command, word_of_operation};
use crate::running::{Run, files_in, lines_in_outputs, problem_with_lines};

/// What earlier versions recorded every map of a target in, under its output directory.
// @lfy def/cli/compile.lfy:compile#compile:compile:d63d4fa0aa6e9549a9eee748c91e1f6aa04d5f823d7362968264e44795a13acf
pub(crate) const LEGACY_MAPS: &str = "source-map.json";

/// The identifier of each target, in scope of --target, whose maps were read from
/// source-map.json rather than from a folder of map files, so that a compile moves them
/// before any unit runs.
///
/// A target whose folder of map files is there had its maps read from that folder, which is
/// where every map recorded since goes, so its source-map.json is not what the plan was made
/// from and nothing is moved out of it.
// @lfy def/cli/compile.lfy:compile#compile:compile:d63d4fa0aa6e9549a9eee748c91e1f6aa04d5f823d7362968264e44795a13acf
pub(crate) fn legacy_targets(workspace: &Workspace, target: Option<&str>) -> Vec<String> {
    let mut found = Vec::new();
    for named in &workspace.targets {
        if target.is_some_and(|target| named.identifier != target) {
            continue;
        }
        if workspace.root.join(MAPS).join(&named.identifier).is_dir() {
            continue;
        }
        if workspace.root.join(&named.output_directory).join(LEGACY_MAPS).is_file() {
            found.push(named.identifier.clone());
        }
    }
    found
}

/// The maps a target kept in source-map.json recorded in each unit's map file in a
/// directory, the target's working directory, and the file removed there once every one is
/// recorded; a unit whose maps could not be recorded leaves the file in place rather than
/// removed, so the maps it holds are not lost.
// @lfy def/cli/compile.lfy:compile#compile:compile:d63d4fa0aa6e9549a9eee748c91e1f6aa04d5f823d7362968264e44795a13acf
// @lfy def/cli/compile.lfy:compile#compile:compile:c5f5abacff471fdeee5c33c45e39bd231d0a70954c1e8d292c875efbc7dc7697
// @lfy def/cli/compile.lfy:compile#compile:compile:b9ad771de6ec1ee6e2812d5335aef919fe9cd8228cb6b2fea9731de46c6dd24e
pub(crate) fn move_legacy_maps(workspace: &Workspace, plan: &Plan, target: &str) {
    let Some(named) = workspace.targets.iter().find(|named| named.identifier == target) else { return };
    let path = workspace.root.join(&named.output_directory).join(LEGACY_MAPS);
    let held = generation::read_source_maps(&path);
    let mut every = true;
    for unit in &plan.units {
        if workspace.targets[unit.target].identifier != target {
            continue;
        }
        let source = &workspace.files[unit.file].path;
        let mine: Vec<SourceMap> =
            held.iter().filter(|map| map.target == target && &map.source == source).cloned().collect();
        if mine.is_empty() {
            continue;
        }
        // @lfy def/cli/compile.lfy:compile#compile:compile:d63d4fa0aa6e9549a9eee748c91e1f6aa04d5f823d7362968264e44795a13acf
        every &= generation::record(workspace, unit, &mine);
    }
    // Every one recorded, the file is removed; one that could not be leaves it in place
    // rather than removed, so the maps it holds are not lost.
    // @lfy def/cli/compile.lfy:compile#compile:compile:b9ad771de6ec1ee6e2812d5335aef919fe9cd8228cb6b2fea9731de46c6dd24e
    if every {
        // @lfy def/cli/compile.lfy:compile#compile:compile:c5f5abacff471fdeee5c33c45e39bd231d0a70954c1e8d292c875efbc7dc7697
        let _ = fs::remove_file(&path);
    }
}

/// Every file of a folder of one target, under the workspace's root, that belongs to no unit
/// of the plan, because its definition file was deleted, moved, or no longer builds for the
/// target, removed.
// @lfy def/cli/compile.lfy:compile#compile:compile:4347f7c42165acde976e4f4fd1045901a022a7affb1faca571f70c981be6c9a4
// @lfy def/cli/compile.lfy:compile#compile:compile:ea7ff5171f6e9c135b3736f46e4d6878cfe8097bd5ca7b45686d9b57a0e0cfb7
pub(crate) fn remove_orphans(workspace: &Workspace, plan: &Plan, target: &str, folder: &str, file_of: fn(&Workspace, &Unit) -> PathBuf) {
    let mine: BTreeSet<PathBuf> = plan
        .units
        .iter()
        .filter(|unit| workspace.targets[unit.target].identifier == target)
        .map(|unit| file_of(workspace, unit))
        .collect();
    let mut found = Vec::new();
    walk_all(&workspace.root.join(folder).join(target), &mut found);
    for path in found {
        // @lfy def/cli/compile.lfy:compile#compile:compile:4347f7c42165acde976e4f4fd1045901a022a7affb1faca571f70c981be6c9a4
        if path.extension().is_some_and(|e| e == "json") && !mine.contains(&path) {
            let _ = fs::remove_file(&path);
        }
    }
}

/// One unit as `elfie_units` lists it: the target, the stem, the reason or up to date, and
/// the stems of its dependencies.
///
/// Without --json the target is muted, the stem is subject, the reason is painted in its own
/// tone, and the dependencies are muted; the words and their order stay as they are, so a
/// script splitting a line on spaces reads the same fields.
// @lfy def/cli/compile.lfy:compile#compile:compile:1158ef5598e98842590c9c237df6cac12c9a46afcab7fb2c9828e80e76f88f9b
// @lfy def/cli/compile.lfy:compile#compile:compile:84bb3a6e0235d5d36cc865a418d3f617e6b697d65fd85324d85cc995e44a3e89
pub(crate) fn unit_line(workspace: &Workspace, plan: &Plan, index: usize, on: bool) -> String {
    let unit = &plan.units[index];
    let reason = unit.reason.map_or_else(|| "up to date".to_string(), |r| r.as_str().to_string());
    let dependencies: Vec<&str> = unit.dependencies.iter().map(|&d| plan.units[d].stem.as_str()).collect();
    // @lfy def/cli/compile.lfy:compile#compile:compile:84bb3a6e0235d5d36cc865a418d3f617e6b697d65fd85324d85cc995e44a3e89
    format!(
        "{} {} {} {}",
        paint(&workspace.targets[unit.target].identifier, Tone::Muted, on),
        paint(&unit.stem, Tone::Subject, on),
        paint(&reason, reason_tone(unit.reason), on),
        paint(&format!("[{}]", dependencies.join(", ")), Tone::Muted, on)
    )
}

/// One of [`Plan::moves`] as a compile prints it: the word move, the target, the stem of the
/// origin, the word into, the stem of the unit that compiles them, the identifiers of the
/// entities moved in brackets, the word for, and the cause.
///
/// Painted, the target and the words move, into, and for are muted, the two stems are
/// subject, and the identifiers and the cause are plain; the words and their order stay as
/// they are either way.
// @lfy def/cli/compile.lfy:compile#compile:compile:0f5a7a1258a11b49d77120109972e47f61867f0f8fe2d59078aa9bfa4ffb1849
// @lfy def/cli/compile.lfy:compile#compile:compile:d76dbb95b25d73fb984ddecdb07366d73e2c932574c9a94208ad9df30c8d8933
pub(crate) fn move_text(workspace: &Workspace, plan: &Plan, moved: &Move, on: bool) -> String {
    let origin = &plan.units[moved.origin];
    // @lfy def/cli/compile.lfy:compile#compile:compile:d76dbb95b25d73fb984ddecdb07366d73e2c932574c9a94208ad9df30c8d8933
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

/// One of [`Plan::moves`] as the one result --json prints for it: the stem of the origin, the
/// target, the stem of the unit that compiles them, the identifiers of the entities moved,
/// and the cause.
// @lfy def/cli/compile.lfy:compile#compile:compile:0f5a7a1258a11b49d77120109972e47f61867f0f8fe2d59078aa9bfa4ffb1849
// @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
pub(crate) fn move_json(workspace: &Workspace, plan: &Plan, moved: &Move) -> serde_json::Value {
    let origin = &plan.units[moved.origin];
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
}

/// One of [`Relocated::relocations`] as a dry run prints it: the word relocate, the target,
/// the entity, the file it left, the word into, the file it is in now, and the count of
/// markers rewritten with the word marker, or markers when it is not 1.
///
/// Painted, the target and the words relocate and into are muted, the two files are subject,
/// and the entity and the count are plain; the words and their order stay as they are either
/// way.
// @lfy def/cli/compile.lfy:compile#compile:compile:5042efb3c946c5366ff884b941565a037c3850a2204c27eacb2e1b0695372035
// @lfy def/cli/compile.lfy:compile#compile:compile:57f98e0f79a657eab8206e1ac47bf236e459d925b355f7534fc575b98b968a1b
pub(crate) fn relocate_text(relocation: &Relocation, on: bool) -> String {
    let word = if relocation.markers == 1 { "marker" } else { "markers" };
    // @lfy def/cli/compile.lfy:compile#compile:compile:57f98e0f79a657eab8206e1ac47bf236e459d925b355f7534fc575b98b968a1b
    format!(
        "{} {} {} {} {} {} {} {word}",
        paint("relocate", Tone::Muted, on),
        paint(&relocation.target, Tone::Muted, on),
        relocation.entity,
        paint(&relocation.origin, Tone::Subject, on),
        paint("into", Tone::Muted, on),
        paint(&relocation.into, Tone::Subject, on),
        relocation.markers
    )
}

/// One of [`Relocated::relocations`] as the one result --json prints for it.
// @lfy def/cli/compile.lfy:compile#compile:compile:5042efb3c946c5366ff884b941565a037c3850a2204c27eacb2e1b0695372035
// @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
pub(crate) fn relocate_json(relocation: &Relocation) -> serde_json::Value {
    serde_json::json!({
        "relocate": relocation.entity,
        "target": relocation.target,
        "origin": relocation.origin,
        "into": relocation.into,
        "markers": relocation.markers,
    })
}

/// What relocating gives kept to the target --target names, when it is given: its
/// relocations, the outputs under its output directory, its source maps, and its review
/// records.
// @lfy def/cli/compile.lfy:compile#compile:compile:1ebd8b096a857a724b950c4f2608560cb59aee8ee82e454e4f2c9b4fb61f7c5e
pub(crate) fn scoped(relocated: Relocated, workspace: &Workspace, target: Option<&str>) -> Relocated {
    let Some(name) = target else { return relocated };
    let directory = workspace
        .targets
        .iter()
        .find(|named| named.identifier == name)
        .map(|named| named.output_directory.clone())
        .unwrap_or_default();
    // @lfy def/cli/compile.lfy:compile#compile:compile:1ebd8b096a857a724b950c4f2608560cb59aee8ee82e454e4f2c9b4fb61f7c5e
    Relocated {
        relocations: relocated.relocations.into_iter().filter(|relocation| relocation.target == name).collect(),
        outputs: relocated
            .outputs
            .into_iter()
            .filter(|output| crate::processes::within(&output.path, &directory))
            .collect(),
        source_maps: relocated.source_maps.into_iter().filter(|map| map.target == name).collect(),
        reviews: relocated.reviews.into_iter().filter(|reviews| reviews.target == name).collect(),
    }
}

/// The target and the source file of each unit every map of whose outputs the relocation
/// made, the unit having had no map of its own among the maps it was read from.
// @lfy def/cli/compile.lfy:compile#compile:compile:56dfba76b54a5c6836f292e001f259f857a7245ccec2e5b571ddfb7f62347513
pub(crate) fn relocated_only(maps: &[SourceMap], relocated: &Relocated) -> BTreeSet<(String, String)> {
    relocated
        .source_maps
        .iter()
        .map(|map| (map.target.clone(), map.source.clone()))
        .filter(|(target, source)| !maps.iter().any(|map| &map.target == target && &map.source == source))
        .collect()
}

/// The moves, then the relocations, then each unit as `elfie_units` lists it, in plan order,
/// and nothing generated: nothing the relocation gives is written either.
// @lfy def/cli/compile.lfy:compile#compile:compile:1158ef5598e98842590c9c237df6cac12c9a46afcab7fb2c9828e80e76f88f9b
// @lfy def/cli/compile.lfy:compile#compile:compile:2e8045c1f86a83462e1d79b1bdf98da4979fc1d0edf4301ad3cf8b989dc12004
// @lfy def/cli/compile.lfy:compile#compile:compile:5d97ccabe3f421ec1dcd897744e41b146dc2b9c49e9b10c2749b0b64f809346a
pub(crate) fn dry_run(
    workspace: &Workspace,
    plan: &Plan,
    relocated: &Relocated,
    target: Option<&str>,
    json: bool,
    on: bool,
) -> ExitCode {
    // The move lines come first, then the relocate lines, then the unit lines.
    // @lfy def/cli/compile.lfy:compile#compile:compile:3fba4385c7b8b84625d1b60c07fcd93cf8655fa31717ba399f3623d31b803900
    // @lfy def/cli/compile.lfy:compile#compile:compile:0f5a7a1258a11b49d77120109972e47f61867f0f8fe2d59078aa9bfa4ffb1849
    for moved in &plan.moves {
        let origin = &plan.units[moved.origin];
        if target.is_some_and(|name| workspace.targets[origin.target].identifier != name) {
            continue;
        }
        if json {
            // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
            println!("{}", move_json(workspace, plan, moved));
        } else {
            // @lfy def/cli/compile.lfy:compile#compile:compile:d76dbb95b25d73fb984ddecdb07366d73e2c932574c9a94208ad9df30c8d8933
            println!("{}", move_text(workspace, plan, moved, on));
        }
    }
    // @lfy def/cli/compile.lfy:compile#compile:compile:5042efb3c946c5366ff884b941565a037c3850a2204c27eacb2e1b0695372035
    // @lfy def/cli/compile.lfy:compile#compile:compile:3fba4385c7b8b84625d1b60c07fcd93cf8655fa31717ba399f3623d31b803900
    for relocation in &relocated.relocations {
        if json {
            // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
            println!("{}", relocate_json(relocation));
        } else {
            // @lfy def/cli/compile.lfy:compile#compile:compile:57f98e0f79a657eab8206e1ac47bf236e459d925b355f7534fc575b98b968a1b
            println!("{}", relocate_text(relocation, on));
        }
    }
    // Each planned unit, in plan order.
    // @lfy def/cli/compile.lfy:compile#compile:compile:1158ef5598e98842590c9c237df6cac12c9a46afcab7fb2c9828e80e76f88f9b
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
    ExitCode::Success
}

/// The stub line of a target's language: the value the setters of its declaration give
/// `stubLine`, read as [`Target::marker_comment`] is read from `markerComment`; nothing when no
/// setter gives it one, and then no line is a stub line.
// @lfy def/cli/compile.lfy:compile#compile:compile:310411e96e27f58a43d127d3470e193be29741813413d0d700495a7648e6299d
// @lfy def/cli/compile.lfy:compile#compile:compile:05a98f1380573aab179458037c363fd33d4604fb69940c131e203ae327ee9388
pub(crate) fn stub_line_of(workspace: &Workspace, target: &Target) -> Option<String> {
    workspace.model.entities[target.declaration]
        .value("stubLine")
        .and_then(elfie_core::model::Value::as_str)
        .map(str::to_string)
}

/// Whether a line, with the white space before and after it removed, reads a stub line, with
/// `{{unit}}` and `{{reason}}` in it standing for any text.
// @lfy def/cli/compile.lfy:compile#compile:compile:310411e96e27f58a43d127d3470e193be29741813413d0d700495a7648e6299d
pub(crate) fn is_stub_line(stub: &str, line: &str) -> bool {
    let line = line.trim();
    // The literal pieces between the placeholders, each in order.
    // @lfy def/cli/compile.lfy:compile#compile:compile:310411e96e27f58a43d127d3470e193be29741813413d0d700495a7648e6299d
    let pieces: Vec<&str> = stub.trim().split("{{unit}}").flat_map(|piece| piece.split("{{reason}}")).collect();
    let (first, rest) = pieces.split_first().expect("split gives at least one piece");
    let Some(rest) = rest.split_last() else { return line == *first };
    let (last, middle) = rest;
    if line.len() < first.len() + last.len() || !line.starts_with(first) || !line.ends_with(last) {
        return false;
    }
    // Any text stands in each gap, so each middle piece found leftmost after the one before
    // it decides whether the line reads the stub line.
    let mut within = &line[first.len()..line.len() - last.len()];
    for piece in middle {
        match within.find(piece) {
            Some(at) => within = &within[at + piece.len()..],
            None => return false,
        }
    }
    true
}

/// The problems of an attempt appended to its instructions, so that the unit is run once
/// more knowing what was wrong.
// @lfy def/cli/compile.lfy:compile#compile:compile:4f06b5aa3049fb148a394aa3261e0dc49c2e9cb564b4604bea58741f2a16204f
// @lfy def/cli/compile.lfy:compile#compile:compile:989f6f309c8ec4cc4f9e9682676ed8016bcfaac8a5f6ccf14d33c1bc0207a0ec
pub(crate) fn append_problems(request: &mut Request, problems: &[String]) {
    if problems.is_empty() {
        return;
    }
    request.instructions.push_str("\n\n## Problems with the previous attempt\n\n");
    for problem in problems {
        request.instructions.push_str(&format!("- {problem}\n"));
    }
}

/// The diagnostics a compile prints when loading found an error: all of them, so a warning
/// stands beside the error that stopped the run. Empty when none of them is an error.
// @lfy def/cli/compile.lfy:compile#compile:compile:f2d15d5c350694370ffb5656be0859f0f69da753546ea57b01ed5e684923d727
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
// @lfy def/cli/compile.lfy:compile#compile:compile:f2d15d5c350694370ffb5656be0859f0f69da753546ea57b01ed5e684923d727
pub(crate) fn print_loaded_diagnostics(workspace: &Workspace, json: bool, on: bool) -> bool {
    let diagnostics = loaded_diagnostics(workspace);
    for diagnostic in &diagnostics {
        print_diagnostic(diagnostic, json, on);
    }
    !diagnostics.is_empty()
}

/// The change sets kept for the targets in scope of --target, by target, as their files
/// under elfie-compile/cache hold them.
// @lfy def/cli/compile.lfy:compile#compile:compile:db9055e3c9db5499ef9db628c8352be669b3c2eb83fc5533541757772f6fae75
pub(crate) fn kept_sets(workspace: &Workspace, target: Option<&str>) -> BTreeMap<String, ChangeSet> {
    workspace
        .targets
        .iter()
        .filter(|named| target.is_none_or(|target| named.identifier == target))
        .filter_map(|named| ChangeSet::load(&workspace.root, &named.identifier).map(|set| (named.identifier.clone(), set)))
        .collect()
}

/// Every source map the map files of each kept change set's copy hold for its target.
// @lfy def/cli/compile.lfy:compile#compile:compile:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
pub(crate) fn maps_of_copies(sets: &BTreeMap<String, ChangeSet>) -> Vec<SourceMap> {
    let mut out = Vec::new();
    for set in sets.values() {
        let mut found = Vec::new();
        walk_all(&Path::new(&set.directory).join(MAPS).join(&set.target), &mut found);
        for path in found.iter().filter(|path| path.extension().is_some_and(|e| e == "json")) {
            out.extend(generation::read_source_maps(path));
        }
    }
    out
}

/// The foreign edits of every change set given that were recorded since their owner was last
/// accepted in it, as `ChangeSet` says.
// @lfy def/cli/compile.lfy:compile#compile:compile:db9055e3c9db5499ef9db628c8352be669b3c2eb83fc5533541757772f6fae75
pub(crate) fn edits_of(sets: &BTreeMap<String, ChangeSet>) -> Vec<ForeignEdit> {
    sets.values().flat_map(|set| set.recorded_since().into_iter().cloned()).collect()
}

/// Every unit a kept change set accepted or reviewed, with the hash and the requirements it
/// has now, planned with the reason the change set holds for it at its place in the plan,
/// even when the plan gives it none, unless a stem given as an argument, or --all, requests
/// it; the requested stems are every stem with --all.
// @lfy def/cli/compile.lfy:compile#compile:compile:7ddc3a3c56212d70e81bbe0dc295510b33a966489a029c6a5c3c3a0907c4b225
pub(crate) fn keep_kept_units(program: &Program, plan: &mut Plan, sets: &BTreeMap<String, ChangeSet>, requested: &[String]) {
    for unit in &mut plan.units {
        if requested.contains(&unit.stem) {
            continue;
        }
        let Some(set) = sets.get(target_of(&program.workspace, unit)) else { continue };
        let Some(held) = set.units.iter().find(|held| held.stem == unit.stem && held.state.is_done()) else {
            continue;
        };
        // @lfy def/cli/compile.lfy:compile#compile:compile:7ddc3a3c56212d70e81bbe0dc295510b33a966489a029c6a5c3c3a0907c4b225
        let hash = generation::source_hash(&unit.text);
        let requirements = generation::source_hash(&requirement_ids_of(program, unit).join("\n"));
        if held.hash == hash && held.requirements == requirements {
            unit.reason = Some(held.reason);
        }
    }
}

/// Plans the units, works each in a copy of its own and merges it into its target's change
/// set, as many at once as --jobs lets, passes each change set through its final gate,
/// reviews the global criteria and tests, and hands the root every change set at once.
// @lfy def/cli/compile.lfy:compile
pub(crate) fn compile(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    // @lfy def/cli/compile.lfy:compile#compile:compile:c0ff8dc13036c4a0c6308abadabef558ad51da34c43aeed7d60f6cc9a97a368c
    let workspace = workspace::load(root);
    let json = invocation.flag("json");
    let style = Paint::of(invocation);
    // The compiler is never handed a program with problems: no compiler runs, nothing is
    // generated, and the code is problems.
    // @lfy def/cli/compile.lfy:compile#compile:compile:f2d15d5c350694370ffb5656be0859f0f69da753546ea57b01ed5e684923d727
    // @lfy def/cli/compile.lfy:compile#compile:compile:3df84098154780d5074fdbf4d1209831b3e3f8bc87b7bc577035c1b63e2f443d
    if print_loaded_diagnostics(&workspace, json, style.out) {
        // @lfy def/cli/compile.lfy:compile#compile:compile:77ae4e6a5e2396d858d173399db2122dd9965d17bc44b4f05613dfae2362f448
        return ExitCode::Problems;
    }
    let target = match target_named(&workspace, invocation) {
        Ok(target) => target,
        Err(code) => return code,
    };
    // With --target the maps of that target alone are read, so no other target's map files
    // are opened.
    // @lfy def/cli/compile.lfy:compile#compile:compile:070e9d726decc495290d01d858a434199ebb3e403601569889127b0066bef9ce
    // @lfy def/cli/compile.lfy:compile#compile:compile:6b9feed8c0c1dd698132e42cc588b0234d6d80924e6e0ecb4b3e6d566b947e8d
    let maps = generation::source_maps_of(&workspace, target.as_deref());
    let legacy = legacy_targets(&workspace, target.as_deref());
    // The plan's foreign edits are those of the kept change set of each target it plans
    // recorded since their owner was last accepted in it.
    // @lfy def/cli/compile.lfy:compile#compile:compile:db9055e3c9db5499ef9db628c8352be669b3c2eb83fc5533541757772f6fae75
    let mut kept = kept_sets(&workspace, target.as_deref());
    drop(workspace);
    // The compile's relocation: relocate of the plan made as the plan below is, but from the
    // source maps the workspace gives, with the root as its directory, called once and before
    // any other plan is made. With --target only what it gives of that target counts.
    // @lfy def/cli/compile.lfy:compile#compile:compile:e8cbd3106b43f34de25194cf3d753b8a20fdf0b93d4277e2513da07e95bdea25
    // @lfy def/cli/compile.lfy:compile#compile:compile:1ebd8b096a857a724b950c4f2608560cb59aee8ee82e454e4f2c9b4fb61f7c5e
    let (first, before) = plan_with(root, &maps, &invocation.arguments, &[], &edits_of(&kept));
    let relocated =
        scoped(generation::relocate(&first, &before, Some(root)), &first.workspace, target.as_deref());
    drop((first, before));
    // The units generated are those of the plan made from the relocated source maps, with the
    // units the last global review found violated, and the stems requested.
    // @lfy def/cli/compile.lfy:compile#compile:compile:db9055e3c9db5499ef9db628c8352be669b3c2eb83fc5533541757772f6fae75
    // A unit whose change set the root never took has its markers in the kept copy.
    // @lfy def/cli/compile.lfy:compile#compile:compile:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
    let beside = maps_of_copies(&kept);
    let planning = planning_of(
        root,
        &relocated.source_maps,
        &invocation.arguments,
        invocation.flag("all"),
        &violated_ids(root),
        &beside,
    );
    let (mut program, mut plan) = planning.plan(root, &edits_of(&kept));
    // @lfy def/cli/compile.lfy:compile#compile:compile:7ddc3a3c56212d70e81bbe0dc295510b33a966489a029c6a5c3c3a0907c4b225
    keep_kept_units(&program, &mut plan, &kept, &planning.requested);
    // A cycle among files that no move resolves has no order to build in: its problem is
    // printed as an error and nothing is generated.
    // @lfy def/cli/compile.lfy:compile#compile:compile:055ed621ccb4cfc04dafc60c4ae42b146676fbac4139bd00ac26ddae87c1ad70
    for problem in &plan.problems {
        print_plan_problem(&program.workspace, problem, json, style.out);
    }
    let cycles = !plan.problems.is_empty();
    // @lfy def/cli/compile.lfy:compile#compile:compile:1158ef5598e98842590c9c237df6cac12c9a46afcab7fb2c9828e80e76f88f9b
    if invocation.flag("dry-run") {
        let code = dry_run(&program.workspace, &plan, &relocated, target.as_deref(), json, style.out);
        return if cycles { ExitCode::Problems } else { code };
    }
    // Nothing is generated, and the code is problems.
    // @lfy def/cli/compile.lfy:compile#compile:compile:e0179e86c25f7960127d204e9f6499cb8cf0495adac3ba8441ff18238b46840d
    if cycles {
        // @lfy def/cli/compile.lfy:compile#compile:compile:d45b61bd01620de4f1c204c2a1a111fa2a8b3b2fff6d63bb3cf7faf41c8f0739
        return ExitCode::Problems;
    }
    let compiler = compiler_command(root);
    let accept_only = invocation.flag("accept");
    // A target's planned units are worked in a change set only when a compiler runs on them.
    // @lfy def/cli/compile.lfy:compile#compile:compile:52dc0204d72665f32b60eef1095ce6a73d89469226840fd8606ff7d622cc4745
    // @lfy def/cli/compile.lfy:compile#compile:compile:d5f565b191777716fd8e847870720c599c23591f1d959c0cb67397a6770f7536
    // @lfy def/cli/compile.lfy:compile#compile:compile:0440d8388eff8352220c16c7b7cbcac23c06b46e2e0c8882f55b1629dc83a76a
    let works = compiler.is_some() && !accept_only;
    // A kept change set none of whose units would keep its state is removed, with its copy
    // and its base, and the plan is made again from the relocated source maps as if it were
    // not kept, so no unit is planned as touched by its foreign edits.
    // @lfy def/cli/compile.lfy:compile#compile:compile:ec2256238e3a7d159a6c973c504ea6c464150e9afdae860ee76850d8941cb16c
    if works {
        let gone = discarded(&program, &plan, &kept, &planning.requested, invocation.flag("all"));
        if !gone.is_empty() {
            for name in &gone {
                if let Some(set) = kept.remove(name)
                    && let Err(error) = remove_change_set(root, &set)
                {
                    complain(style.err, Some(&ChangeSet::file(root, name).display().to_string()), &error.to_string());
                }
            }
            (program, plan) = planning.plan(root, &edits_of(&kept));
            keep_kept_units(&program, &mut plan, &kept, &planning.requested);
        }
    }
    let reporter = Reporter::new(json, style, 0, log_paths(root));
    let mut run = Run::new(program, plan, planning, reporter, invocation);
    // @lfy def/cli/compile.lfy:compile#compile:compile:56dfba76b54a5c6836f292e001f259f857a7245ccec2e5b571ddfb7f62347513
    run.relocated_only = relocated_only(&maps, &relocated);
    // @lfy def/cli/compile.lfy:compile#compile:compile:bdff677c0d440152d6dce214b68c273f5922cbd55390da1c6084e2013d1a3f4e
    run.relocation = relocated;
    run.count_done();
    // The first progress line of a compile is planned, counting the units planned and the
    // moves, whether anything was planned or not, so that a compile with nothing to do still
    // reads as one run.
    // @lfy def/cli/compile.lfy:compile#compile:compile:3b7ff1cb8c57ebc94c72273c9e46802fa9a2881815174d5e72e44b93e5ad00ea
    // @lfy def/cli/compile.lfy:compile#compile:compile:016ab6d6e7fb03c113d8693d81ea927c68a7ee23a2750efeff879652d9859d00
    let total = run.reporter.total();
    let planned = format!("{total} units planned, {} moves", run.plan.moves.len());
    run.reporter.report(Step::Planned, None, &planned);
    // The move lines follow the planned line, each indented four spaces; with --json each
    // move is one result of its own.
    // @lfy def/cli/compile.lfy:compile#compile:compile:99a4adc185fa87df9a6aacaa34f84261addbd0756d5dc199405c5bc004f3e357
    // @lfy def/cli/compile.lfy:compile#compile:compile:0f5a7a1258a11b49d77120109972e47f61867f0f8fe2d59078aa9bfa4ffb1849
    if json {
        for moved in &run.plan.moves {
            // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
            let line = move_json(&run.program.workspace, &run.plan, moved).to_string();
            println!("{line}");
            run.reporter.append(&line);
        }
    } else {
        for moved in &run.plan.moves {
            let line = move_text(&run.program.workspace, &run.plan, moved, style.out);
            println!("{INDENT}{line}");
            let plain = move_text(&run.program.workspace, &run.plan, moved, false);
            run.reporter.append(&format!("{INDENT}{plain}"));
        }
    }
    // One relocated line for each target the compile plans that the relocation moved code of,
    // after the planned line and its move lines.
    // @lfy def/cli/compile.lfy:compile#compile:compile:be6f670ec86351b9c898da8d21d0741aadee997be9c0c07bfeb4504ac8af962c
    run.report_relocations();
    // @lfy def/cli/compile.lfy:compile#compile:compile:5b05d045fbdda6a51d4ac309b97ddac4c12d0a0e967117d6973df4878cc1bc96
    if total == 0 && !json {
        println!("{}", paint("every unit is up to date", Tone::Success, style.out));
    }
    if works {
        run.prepare_change_sets(kept);
    }
    // Before any unit runs, in the working directory of each planned target, the maps it kept
    // in source-map.json are moved into the units' map files and every map file and review
    // record file belonging to no unit of the plan is removed.
    // @lfy def/cli/compile.lfy:compile#compile:compile:d63d4fa0aa6e9549a9eee748c91e1f6aa04d5f823d7362968264e44795a13acf
    // @lfy def/cli/compile.lfy:compile#compile:compile:4347f7c42165acde976e4f4fd1045901a022a7affb1faca571f70c981be6c9a4
    // @lfy def/cli/compile.lfy:compile#compile:compile:ea7ff5171f6e9c135b3736f46e4d6878cfe8097bd5ca7b45686d9b57a0e0cfb7
    run.tidy(&legacy);
    match (&compiler, accept_only) {
        // @lfy def/cli/compile.lfy:compile#compile:compile:c51341f34278d51ccda0b1a75908b211f46b7db736525ed29c88351db922c346
        // @lfy def/cli/compile.lfy:compile#compile:compile:d496f4e84cd3af18e7e1560f56d42b8d95af38099dfe02467aa01339ad1d8434
        (_, true) => {
            let root = run.root();
            let relocated = run.relocation.clone();
            run.write_relocation(&root, &relocated);
            run.accept_on_disk();
        }
        // Nothing the relocation gives is written without a compiler.
        // @lfy def/cli/compile.lfy:compile#compile:compile:af5bc490a429019de13aaa5043d30cc6c3e41965559e13e6116286205a08801e
        // @lfy def/cli/compile.lfy:compile#compile:compile:d5f565b191777716fd8e847870720c599c23591f1d959c0cb67397a6770f7536
        (None, false) => run.write_requests(),
        (Some(command), false) => {
            run.ready_change_sets(command);
            run.run_units(command);
        }
    }
    run.close_the_compile();
    // Every session of this compile is ended and the server stopped, after the last run has
    // returned and before the command returns, whatever the outcome.
    // @lfy def/cli/compile.lfy:compile#compile:compile:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
    run.agent.server.stop();
    // The last progress line of a compile is finished.
    // @lfy def/cli/compile.lfy:compile#compile:compile:4da6fc0ef0b92bdd309365b39295a5cc5de6e9d3296556c8da4e7fc7e1c11920
    run.finish();
    // No unit planned, or the root took every change set, is success; a unit rejected,
    // blocked, asking a question, or failed, or a final gate, the global review, or the merge
    // failing, is problems; a command that could not run is failure.
    // @lfy def/cli/compile.lfy:compile#compile:compile:3e779a9b6f0378e44c4ad48cec920b4a7cc005d5fba9f728bcdb2b295a89a2e6
    // @lfy def/cli/compile.lfy:compile#compile:compile:3d1e2128231c0dd4a623216ed6845b9bfdce55b3a624742046eb528f6cff68dd
    // @lfy def/cli/compile.lfy:compile#compile:compile:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
    run.code
}

/// The targets whose kept change set no unit of would keep its state, as `ChangeSet` says,
/// a unit a stem given as an argument, or --all, requests running again as one whose
/// definitions changed.
// @lfy def/cli/compile.lfy:compile#compile:compile:ec2256238e3a7d159a6c973c504ea6c464150e9afdae860ee76850d8941cb16c
// @lfy def/cli/compile.lfy:compile#compile:compile:3ac75787bb8128ea850e5a2f38fe77bd7b84d3e981ba0b5b14f2e95f74b18288
pub(crate) fn discarded(
    program: &Program,
    plan: &Plan,
    kept: &BTreeMap<String, ChangeSet>,
    requested: &[String],
    all: bool,
) -> Vec<String> {
    let mut gone = Vec::new();
    for (name, set) in kept {
        let now = units_now(program, plan, name, requested, all);
        // A change set whose copy is gone keeps nothing either.
        // @lfy def/cli/compile.lfy:compile#compile:compile:ec2256238e3a7d159a6c973c504ea6c464150e9afdae860ee76850d8941cb16c
        if now.is_empty() || set.kept(&now) == 0 || !Path::new(&set.directory).is_dir() {
            gone.push(name.clone());
        }
    }
    gone
}

/// A target's planned units as a change set holds them, in plan order, each with the hash
/// and the requirements it has now; a unit a stem given as an argument, or --all, requests
/// with none, so that a kept change set runs it again as one whose definitions changed.
// @lfy def/cli/compile.lfy:compile#compile:compile:52dc0204d72665f32b60eef1095ce6a73d89469226840fd8606ff7d622cc4745
// @lfy def/cli/compile.lfy:compile#compile:compile:3ac75787bb8128ea850e5a2f38fe77bd7b84d3e981ba0b5b14f2e95f74b18288
pub(crate) fn units_now(program: &Program, plan: &Plan, target: &str, requested: &[String], all: bool) -> Vec<ChangeSetUnit> {
    plan.units
        .iter()
        .filter(|unit| target_of(&program.workspace, unit) == target)
        .filter_map(|unit| {
            let reason = unit.reason?;
            // @lfy def/cli/compile.lfy:compile#compile:compile:3ac75787bb8128ea850e5a2f38fe77bd7b84d3e981ba0b5b14f2e95f74b18288
            if all || requested.contains(&unit.stem) {
                return Some(ChangeSetUnit::planned(&unit.stem, reason, "", ""));
            }
            let hash = generation::source_hash(&unit.text);
            let requirements = generation::source_hash(&requirement_ids_of(program, unit).join("\n"));
            Some(ChangeSetUnit::planned(&unit.stem, reason, &hash, &requirements))
        })
        .collect()
}

impl Run {
    /// The change set of each target with a planned unit, in the order of the targets: a
    /// kept one resumed, its copy brought up to the root and one resuming line counting the
    /// units it keeps and the units that run again, or one made new, its copy and base made
    /// before its first unit runs. A kept change set for a target with no planned unit is
    /// removed, with its copy and its base, unless --target names another target.
    // @lfy def/cli/compile.lfy:compile#compile:compile:52dc0204d72665f32b60eef1095ce6a73d89469226840fd8606ff7d622cc4745
    // @lfy def/cli/compile.lfy:compile#compile:compile:46046c83c1cfa40ed79495fde1ef0777d6db2402a03b330c3064d14c34546303
    // @lfy def/cli/compile.lfy:compile#compile:compile:54faac0351075e5f797a3b89b57d4af904f0f8a00cf097ab716767e08566ab00
    // @lfy def/cli/compile.lfy:compile#compile:compile:489059c8a497ac06d146f6905d881772dce1f6e60f48adf6f5bd5b6ca509bdc2
    // @lfy def/cli/compile.lfy:compile#compile:compile:b42383729e83012724e5ae4eb03736c20f286e73e36e0bfb45b2affcd6832907
    pub(crate) fn prepare_change_sets(&mut self, mut kept: BTreeMap<String, ChangeSet>) {
        let root = self.root();
        let targets: Vec<Target> = self.program.workspace.targets.clone();
        for named in &targets {
            if self.target.as_deref().is_some_and(|target| target != named.identifier) {
                continue;
            }
            // --all has requested every stem already.
            let now = units_now(&self.program, &self.plan, &named.identifier, &self.planning.requested, false);
            // A kept change set for a target the plan holds no planned unit of is removed.
            // @lfy def/cli/compile.lfy:compile#compile:compile:b42383729e83012724e5ae4eb03736c20f286e73e36e0bfb45b2affcd6832907
            if now.is_empty() {
                if let Some(set) = kept.remove(&named.identifier)
                    && let Err(error) = remove_change_set(&root, &set)
                {
                    complain(self.style.err, Some(&set.directory), &error.to_string());
                }
                continue;
            }
            let set = match kept.remove(&named.identifier) {
                // Its copy and its base are used as they are, the copy brought up to the root.
                // @lfy def/cli/compile.lfy:compile#compile:compile:54faac0351075e5f797a3b89b57d4af904f0f8a00cf097ab716767e08566ab00
                // @lfy def/cli/compile.lfy:compile#compile:compile:489059c8a497ac06d146f6905d881772dce1f6e60f48adf6f5bd5b6ca509bdc2
                Some(mut set) => {
                    set.resume(&now);
                    if let Err(error) = sync_copy(&root, Path::new(&set.directory), named) {
                        complain(self.style.err, Some(&set.directory), &error.to_string());
                        self.worsen(ExitCode::Failure);
                    }
                    let keeps = set.units.iter().filter(|unit| unit.state.is_done()).count();
                    let again = set.units.len() - keeps;
                    // @lfy def/cli/compile.lfy:compile#compile:compile:54faac0351075e5f797a3b89b57d4af904f0f8a00cf097ab716767e08566ab00
                    // @lfy def/cli/compile.lfy:compile#compile:compile:b6e79b3ca5fa492391881efa95a4994ac3f869e7623e0954b6788a6bc1a06e2a
                    let message = format!("{}: {keeps} units kept, {again} run again", named.identifier);
                    self.reporter.report(Step::Resuming, None, &message);
                    set
                }
                // A new change set: its copy and its base made before its first unit runs.
                // @lfy def/cli/compile.lfy:compile#compile:compile:46046c83c1cfa40ed79495fde1ef0777d6db2402a03b330c3064d14c34546303
                None => match self.new_change_set(named, now) {
                    Some(set) => set,
                    None => continue,
                },
            };
            self.sets.insert(named.identifier.clone(), set);
            self.save(&named.identifier);
        }
        self.count_done();
    }

    /// A target's change set made new, holding the units given as planned: its copy and its
    /// base made under elfie-compile/cache before its first unit runs. Nothing, and the compile
    /// stopped with the code failure, when the copy cannot be made.
    // @lfy def/cli/compile.lfy:compile#compile:compile:46046c83c1cfa40ed79495fde1ef0777d6db2402a03b330c3064d14c34546303
    pub(crate) fn new_change_set(&mut self, named: &Target, now: Vec<ChangeSetUnit>) -> Option<ChangeSet> {
        let root = self.root();
        let directory = match make_copy(&root, named) {
            Ok(directory) => directory,
            Err(error) => {
                let at = copy_of(&root, &named.identifier).display().to_string();
                complain(self.style.err, Some(&at), &error.to_string());
                self.worsen(ExitCode::Failure);
                self.halted = true;
                return None;
            }
        };
        // A planned unit's hash and requirements are empty until it is accepted.
        let units = now.into_iter().map(|held| ChangeSetUnit::planned(&held.stem, held.reason, "", "")).collect();
        Some(ChangeSet {
            target: named.identifier.clone(),
            directory: directory.to_string_lossy().into_owned(),
            units,
            foreign_edits: Vec::new(),
        })
    }

    /// Before any unit runs, in the working directory of each planned target: the maps it
    /// kept in source-map.json recorded in its units' map files there and that file removed,
    /// every map file belonging to no unit of the plan removed, and, when a verifier is to
    /// review, every review record file belonging to no unit of the plan removed. With
    /// --accept no review record file changes, so none is removed either.
    // @lfy def/cli/compile.lfy:compile#compile:compile:d63d4fa0aa6e9549a9eee748c91e1f6aa04d5f823d7362968264e44795a13acf
    // @lfy def/cli/compile.lfy:compile#compile:compile:4347f7c42165acde976e4f4fd1045901a022a7affb1faca571f70c981be6c9a4
    // @lfy def/cli/compile.lfy:compile#compile:compile:ea7ff5171f6e9c135b3736f46e4d6878cfe8097bd5ca7b45686d9b57a0e0cfb7
    // @lfy def/cli/compile.lfy:compile#compile:compile:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
    pub(crate) fn tidy(&mut self, legacy: &[String]) {
        // The targets the plan holds a unit of, in scope of --target.
        let planned: BTreeSet<String> = self
            .plan
            .units
            .iter()
            .map(|unit| target_of(&self.program.workspace, unit).to_string())
            .filter(|name| self.target.as_deref().is_none_or(|target| target == name))
            .collect();
        let reviews = self.verifies() && !self.accept_only;
        let plan = self.plan.clone();
        for name in planned {
            // A target's working directory is its change set's copy, and the root when the
            // compile works no change set; while it works some, nothing of the root changes but
            // through the root taking them.
            // @lfy def/cli/compile.lfy:compile#compile:compile:620fa0faf525e60e5d31d15d5f9c42d2193013818b1cf267a45bff765761c588
            let directory = match self.sets.get(&name) {
                Some(set) => PathBuf::from(&set.directory),
                None if self.sets.is_empty() => self.root(),
                None => continue,
            };
            let program = self.program_at(&directory);
            // @lfy def/cli/compile.lfy:compile#compile:compile:d63d4fa0aa6e9549a9eee748c91e1f6aa04d5f823d7362968264e44795a13acf
            if legacy.contains(&name) {
                move_legacy_maps(&program.workspace, &plan, &name);
            }
            // @lfy def/cli/compile.lfy:compile#compile:compile:4347f7c42165acde976e4f4fd1045901a022a7affb1faca571f70c981be6c9a4
            remove_orphans(&program.workspace, &plan, &name, MAPS, generation::map_file);
            // @lfy def/cli/compile.lfy:compile#compile:compile:ea7ff5171f6e9c135b3736f46e4d6878cfe8097bd5ca7b45686d9b57a0e0cfb7
            if reviews {
                remove_orphans(&program.workspace, &plan, &name, REVIEWS, generation::review_file);
            }
        }
    }

    /// One relocated progress line for each target the compile plans whose code the compile's
    /// relocation moved: the target, then the counts of the entities relocated, the markers
    /// rewritten, and the review records moved, each with its word.
    // @lfy def/cli/compile.lfy:compile#compile:compile:be6f670ec86351b9c898da8d21d0741aadee997be9c0c07bfeb4504ac8af962c
    // @lfy def/cli/compile.lfy:compile#compile:compile:b6e79b3ca5fa492391881efa95a4994ac3f869e7623e0954b6788a6bc1a06e2a
    pub(crate) fn report_relocations(&mut self) {
        let planned: BTreeSet<String> =
            self.planned_units().iter().map(|&unit| self.target_of_unit(unit).identifier.clone()).collect();
        let targets: Vec<String> = self.workspace().targets.iter().map(|named| named.identifier.clone()).collect();
        for name in targets {
            let relocations: Vec<&Relocation> =
                self.relocation.relocations.iter().filter(|relocation| relocation.target == name).collect();
            if relocations.is_empty() || !planned.contains(&name) {
                continue;
            }
            let markers: usize = relocations.iter().map(|relocation| relocation.markers).sum();
            let records = self.records_moved(&name);
            let counted = |count: usize, one: &str, many: &str| format!("{count} {}", if count == 1 { one } else { many });
            // @lfy def/cli/compile.lfy:compile#compile:compile:be6f670ec86351b9c898da8d21d0741aadee997be9c0c07bfeb4504ac8af962c
            let message = format!(
                "{name}: {}, {}, {}",
                counted(relocations.len(), "entity", "entities"),
                counted(markers, "marker", "markers"),
                counted(records, "review record", "review records")
            );
            self.reporter.report(Step::Relocated, None, &message);
        }
    }

    /// How many review records the compile's relocation moved for a target: those of its
    /// [`Relocated::reviews`] that the review record file of their unit does not hold for it
    /// now.
    // @lfy def/cli/compile.lfy:compile#compile:compile:be6f670ec86351b9c898da8d21d0741aadee997be9c0c07bfeb4504ac8af962c
    pub(crate) fn records_moved(&self, target: &str) -> usize {
        let mut moved = 0;
        for reviews in self.relocation.reviews.iter().filter(|reviews| reviews.target == target) {
            let Some(unit) = self
                .plan
                .units
                .iter()
                .find(|unit| unit.stem == reviews.stem && self.workspace().targets[unit.target].identifier == target)
            else {
                continue;
            };
            let held: BTreeSet<String> =
                generation::reviews_of(&self.program, unit).into_iter().map(|record| record.id).collect();
            moved += reviews.records.iter().filter(|record| !held.contains(&record.id)).count();
        }
        moved
    }

    /// What a relocation gives written in a directory, the root or a change set's copy: each
    /// of its outputs at its path there, the maps of each unit whose maps it changed recorded
    /// there by `record`, and the records of each of its reviews recorded there for its unit by
    /// `recordReviews`, both given the workspace with that directory as its root.
    // @lfy def/cli/compile.lfy:compile#compile:compile:fe55c99f3237d5145588322f31713c7b917233c11019798ddb980a89ddaf713a
    // @lfy def/cli/compile.lfy:compile#compile:compile:d496f4e84cd3af18e7e1560f56d42b8d95af38099dfe02467aa01339ad1d8434
    pub(crate) fn write_relocation(&mut self, directory: &Path, relocated: &Relocated) {
        // @lfy def/cli/compile.lfy:compile#compile:compile:fe55c99f3237d5145588322f31713c7b917233c11019798ddb980a89ddaf713a
        for output in &relocated.outputs {
            let path = directory.join(&output.path);
            // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
            let written = path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(&path, &output.text));
            if let Err(error) = written {
                complain(self.style.err, Some(&output.path), &error.to_string());
                self.worsen(ExitCode::Failure);
            }
        }
        let plan = self.plan.clone();
        // The maps of each unit whose maps the relocation changed.
        // @lfy def/cli/compile.lfy:compile#compile:compile:fe55c99f3237d5145588322f31713c7b917233c11019798ddb980a89ddaf713a
        // @lfy def/cli/compile.lfy:compile#compile:compile:d496f4e84cd3af18e7e1560f56d42b8d95af38099dfe02467aa01339ad1d8434
        for unit in 0..plan.units.len() {
            let target = self.target_of_unit(unit).identifier.clone();
            if self.target.as_deref().is_some_and(|named| named != target) {
                continue;
            }
            let source = self.workspace().files[plan.units[unit].file].path.clone();
            let now: Vec<SourceMap> = relocated
                .source_maps
                .iter()
                .filter(|map| map.target == target && map.source == source)
                .cloned()
                .collect();
            if now.is_empty() || now == self.maps_in(unit, directory) {
                continue;
            }
            let program = self.program_at(directory);
            if !generation::record(&program.workspace, &plan.units[unit], &now) {
                let path = generation::map_file(&program.workspace, &plan.units[unit]).display().to_string();
                complain(self.style.err, Some(&path), "could not be written");
                self.worsen(ExitCode::Failure);
            }
        }
        // The records of each of its reviews, for its unit.
        // @lfy def/cli/compile.lfy:compile#compile:compile:fe55c99f3237d5145588322f31713c7b917233c11019798ddb980a89ddaf713a
        // @lfy def/cli/compile.lfy:compile#compile:compile:d496f4e84cd3af18e7e1560f56d42b8d95af38099dfe02467aa01339ad1d8434
        for reviews in &relocated.reviews {
            let Some(unit) = plan
                .units
                .iter()
                .position(|unit| unit.stem == reviews.stem && self.workspace().targets[unit.target].identifier == reviews.target)
            else {
                continue;
            };
            let program = self.program_at(directory);
            if !generation::record_reviews(&program.workspace, &plan.units[unit], &reviews.records) {
                let path = generation::review_file(&program.workspace, &plan.units[unit]).display().to_string();
                complain(self.style.err, Some(&path), "could not be written");
                self.worsen(ExitCode::Failure);
            }
        }
    }

    /// Every change set of the compile readied before its first unit starts: the relocation
    /// of the plan the units are generated from, with its copy as the directory, written there;
    /// every unit it keeps accepted verified there, when a verifier is to review; then its
    /// start made new from its own files as its copy holds them, every unit's record of what
    /// its last merge wrote emptied, and what those merges kept removed. The units the compile
    /// runs are then those of its first plan, each waiting for the units its plan gives it.
    // @lfy def/cli/compile.lfy:compile#compile:compile:fe55c99f3237d5145588322f31713c7b917233c11019798ddb980a89ddaf713a
    // @lfy def/cli/compile.lfy:compile#compile:compile:e6f63c4110436ce781da7a0d7e40aa3ae7cd30fdadbe53ba4e4743073c700a40
    // @lfy def/cli/compile.lfy:compile#compile:compile:d469706755276e361e5ad9c3965b1dd65287850e7c3bdbc70bd70d9c453d4caa
    // @lfy def/cli/compile.lfy:compile#compile:compile:42d392d488315c356a8aec4d1df3e5316914fd7797f15f57772a074b0e121165
    // @lfy def/cli/compile.lfy:compile#compile:compile:8cf4943ef6637dd81797621b8abc048edcc7781a4f19546fbe99863339043826
    pub(crate) fn ready_change_sets(&mut self, command: &str) {
        let root = self.root();
        let targets: Vec<Target> = self.program.workspace.targets.clone();
        for named in &targets {
            let Some(copy) = self.sets.get(&named.identifier).map(|set| PathBuf::from(&set.directory)) else { continue };
            // The relocation of the plan the units are generated from, in the copy.
            // @lfy def/cli/compile.lfy:compile#compile:compile:fe55c99f3237d5145588322f31713c7b917233c11019798ddb980a89ddaf713a
            let plan = self.plan.clone();
            let found = generation::relocate(self.program_at(&copy), &plan, Some(&copy));
            let relocated = scoped(found, &self.program.workspace, Some(&named.identifier));
            self.write_relocation(&copy, &relocated);
            // Every unit it keeps accepted verified in its copy, once the relocation is written
            // and before any of its units starts.
            // @lfy def/cli/compile.lfy:compile#compile:compile:e6f63c4110436ce781da7a0d7e40aa3ae7cd30fdadbe53ba4e4743073c700a40
            if self.verifies() {
                let kept: Vec<usize> = self
                    .planned_units()
                    .into_iter()
                    .filter(|&unit| self.target_of_unit(unit).identifier == named.identifier)
                    .filter(|&unit| self.state_of(unit) == UnitState::Accepted)
                    .collect();
                for unit in kept {
                    if self.halted {
                        return;
                    }
                    if let Some(stop) = self.verify_kept(unit, command) {
                        self.stopped_unit(unit, stop);
                    }
                }
            }
            if self.halted {
                return;
            }
            // Its start, made new, holding its own files as its copy holds them now; every
            // unit's record of its last merge emptied, and what those merges kept removed.
            // @lfy def/cli/compile.lfy:compile#compile:compile:d469706755276e361e5ad9c3965b1dd65287850e7c3bdbc70bd70d9c453d4caa
            if let Err(error) = crate::copies::make_start(&root, &copy, named) {
                complain(self.style.err, Some(&copy.display().to_string()), &error.to_string());
                self.worsen(ExitCode::Failure);
                self.halted = true;
                return;
            }
            if let Some(set) = self.sets.get_mut(&named.identifier) {
                for held in &mut set.units {
                    held.merged.clear();
                }
            }
            let _ = crate::copies::remove_directory(&crate::copies::merges_of_target(&root, &named.identifier));
            self.save(&named.identifier);
        }
        // A unit a kept change set holds accepted or reviewed counts as merged; every other
        // unit is of the compile's first plan, waiting for the units of its waitsFor.
        // @lfy def/cli/compile.lfy:compile#compile:compile:42d392d488315c356a8aec4d1df3e5316914fd7797f15f57772a074b0e121165
        // @lfy def/cli/compile.lfy:compile#compile:compile:8cf4943ef6637dd81797621b8abc048edcc7781a4f19546fbe99863339043826
        for unit in self.planned_units() {
            if self.set_of(unit).is_none() {
                continue;
            }
            if self.state_of(unit).is_done() {
                self.schedule.merged.insert(unit);
                self.schedule.at_start.insert(self.stem(unit));
                continue;
            }
            let waits: Vec<usize> = self.plan.units[unit]
                .waits_for
                .iter()
                .filter_map(|stem| self.plan.units.iter().position(|other| &other.stem == stem && other.target == self.plan.units[unit].target))
                .collect();
            self.schedule.first.insert(unit, waits);
        }
    }

    /// The request of each planned unit, made for no hosted session from the root, written
    /// to `elfie-requests/<name>.md` under the root for a compiler run by hand.
    // @lfy def/cli/compile.lfy:compile#compile:compile:af5bc490a429019de13aaa5043d30cc6c3e41965559e13e6116286205a08801e
    pub(crate) fn write_requests(&mut self) {
        let root = self.root();
        for unit in self.planned_units() {
            let stem = self.stem(unit);
            // @lfy def/cli/compile.lfy:compile#compile:compile:0e1f26aad48b4a2e79096c71738d1754b0ac48ecec6dc143f7bb9c5a1e48987d
            self.recover(unit);
            let reason = self.plan.units[unit].reason.map_or_else(|| "up to date".to_string(), |r| r.as_str().to_string());
            self.reporter.report(Step::Requesting, Some(&stem), &reason);
            // @lfy def/cli/compile.lfy:compile#compile:compile:af5bc490a429019de13aaa5043d30cc6c3e41965559e13e6116286205a08801e
            let (request, _) = self.request_of(unit, &root, false, &[]);
            if let Some(path) = self.write_note(&stem, "md", &request.instructions)
                && !self.json
            {
                println!("wrote {}", relative(&root, &path));
            }
        }
    }

    /// No compiler is run: the outputs already in the root of each planned unit, in plan
    /// order, are checked with `accept` of the unit's request, written back normalized, and
    /// recorded when accepted. A verdict holding a foreign edit rejects the unit, one problem
    /// per edit, since --accept records no foreign edit.
    // @lfy def/cli/compile.lfy:compile#compile:compile:c51341f34278d51ccda0b1a75908b211f46b7db736525ed29c88351db922c346
    // @lfy def/cli/compile.lfy:compile#compile:compile:a3ff4cc29185e0a608fd124eb6e98d73490a313012922426815ffdaaf23e2e2a
    // @lfy def/cli/compile.lfy:compile#compile:compile:bc92c42f122f408df6d607e49b325266342f43d2432cc4b810428d20b881f68f
    // @lfy def/cli/compile.lfy:compile#compile:compile:716c12d8173223d984bf23488c69bcafe1c34d0dfe121ff09b38ac8826ccee47
    // @lfy def/cli/compile.lfy:compile#compile:compile:506c952941bcde9e41666d0723255fd008b8fe51dfa594b03536c3f01e6cfef9
    // @lfy def/cli/compile.lfy:compile#compile:compile:c65e5aa6168e9f62b98f454af939726af67cc464dfa4dcadffec41cbf6e60044
    pub(crate) fn accept_on_disk(&mut self) {
        // Every output is read and written in the root.
        // @lfy def/cli/compile.lfy:compile#compile:compile:0440d8388eff8352220c16c7b7cbcac23c06b46e2e0c8882f55b1629dc83a76a
        let root = self.root();
        for unit in self.planned_units() {
            let stem = self.stem(unit);
            // Its previous source is recovered once, before anything else is done for it.
            // @lfy def/cli/compile.lfy:compile#compile:compile:0e1f26aad48b4a2e79096c71738d1754b0ac48ecec6dc143f7bb9c5a1e48987d
            self.recover(unit);
            // Before its outputs are checked, the markers of its reworded criteria and tests
            // name the ids they have now, written at their paths in the root.
            // @lfy def/cli/compile.lfy:compile#compile:compile:a4ad38d7489ae10f9fe2c66322466508785c0017b210f46370cb10dbbbc8908b
            self.reword_outputs(unit, &root);
            // Nothing reads these instructions: with --accept no compiler is run and no
            // session is hosted, so the request is for no hosted session.
            // @lfy def/cli/compile.lfy:compile#compile:compile:c51341f34278d51ccda0b1a75908b211f46b7db736525ed29c88351db922c346
            let (request, _) = self.request_of(unit, &root, false, &[]);
            // @lfy def/cli/compile.lfy:compile#compile:compile:a3ff4cc29185e0a608fd124eb6e98d73490a313012922426815ffdaaf23e2e2a
            let verdict = self.verdict_of(unit, &request, &root, None, &BTreeMap::new());
            // @lfy def/cli/compile.lfy:compile#compile:compile:506c952941bcde9e41666d0723255fd008b8fe51dfa594b03536c3f01e6cfef9
            let problems: Vec<String> = if !verdict.accepted {
                verdict.problems.clone()
            } else {
                verdict
                    .foreign_edits
                    .iter()
                    .map(|edit| {
                        format!(
                            "{} edits {} of {}, and --accept records no foreign edit",
                            stem, edit.path, edit.owner
                        )
                    })
                    .collect()
            };
            // @lfy def/cli/compile.lfy:compile#compile:compile:c65e5aa6168e9f62b98f454af939726af67cc464dfa4dcadffec41cbf6e60044
            if !problems.is_empty() {
                self.report_rejection(unit, &problems);
                self.rejected += 1;
                self.set_state(unit, UnitState::Rejected);
                self.worsen(ExitCode::Problems);
                continue;
            }
            // @lfy def/cli/compile.lfy:compile#compile:compile:bc92c42f122f408df6d607e49b325266342f43d2432cc4b810428d20b881f68f
            if !self.write_back(unit, &verdict, &root) {
                continue;
            }
            self.reporter.report(Step::Accepted, Some(&stem), &format!("{} outputs", verdict.outputs.len()));
            // @lfy def/cli/compile.lfy:compile#compile:compile:716c12d8173223d984bf23488c69bcafe1c34d0dfe121ff09b38ac8826ccee47
            self.record_maps(unit, &verdict.source_maps, &root, false);
            self.set_state(unit, UnitState::Accepted);
        }
    }

    /// What follows the units of a compile: when every planned unit of every change set is
    /// accepted or reviewed and the compile did not stop, the final gate of each change set,
    /// the global review, and the merge; the units a first violated global review plans run
    /// again before the final gates run again and the global review runs once more.
    // @lfy def/cli/compile.lfy:compile#compile:compile:85a283dc1f225f02956fbed8a23688bb8b6f297fdc8bea08fed48d08fd0de9b5
    // @lfy def/cli/compile.lfy:compile#compile:compile:f2d95b937709d596c90877c72c560452b08ca12e639e6792b8be4919cd63a3f9
    // @lfy def/cli/compile.lfy:compile#compile:compile:afe98c81a441acd8328abb1bd1e4139cb546203a7431be38e71303cf11efb5ea
    pub(crate) fn close_the_compile(&mut self) {
        let mut round = 1;
        loop {
            // A unit stopped without being accepted, or was skipped: no final gate, global
            // review, or merge runs, and every change set is kept.
            // @lfy def/cli/compile.lfy:compile#compile:compile:85a283dc1f225f02956fbed8a23688bb8b6f297fdc8bea08fed48d08fd0de9b5
            if self.halted || !self.stopped.is_empty() || !self.skipped.is_empty() {
                return;
            }
            // @lfy def/cli/compile.lfy:compile#compile:compile:f2d95b937709d596c90877c72c560452b08ca12e639e6792b8be4919cd63a3f9
            if !self.final_gates() {
                return;
            }
            // @lfy def/cli/compile.lfy:compile#compile:compile:0b09728c30bb0edb3d089358237c8d4a4b44e18393eb5824b8af1001750100e0
            let violated = self.review_globally();
            // An agent server that could not be hosted for the global review stopped the
            // compile with the code failure: the root takes no change set.
            // @lfy def/cli/compile.lfy:compile#compile:compile:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
            if self.halted {
                self.gate_failed = true;
                return;
            }
            // A global review that holds no violated review passes, and the root takes every
            // change set.
            // @lfy def/cli/compile.lfy:compile#compile:compile:33118ef91e83ccc906ceb521dc58ca5269a50860296c00d79f346c82a4a42dc6
            // @lfy def/cli/compile.lfy:compile#compile:compile:d65259b76766c2e72bd7a2d4077d7121611026a93aacf0a91ba6d86e97ddbe6d
            if violated.is_empty() {
                self.take_change_sets();
                return;
            }
            // The second violated global review stops the compile: the root takes no change
            // set, every one is kept, and the code is problems; the next compile plans those
            // units with reason violated.
            // @lfy def/cli/compile.lfy:compile#compile:compile:7151ea4a586484bae8988155ff4f9c3c2522dc8682ecefe1fbbe3d5ad7a87ec4
            // @lfy def/cli/compile.lfy:compile#compile:compile:96dd6a88ebe0003d78a67421241c801e1f0e0faf8ced5a63a7825b67a31a2729
            // @lfy def/cli/compile.lfy:compile#compile:compile:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
            if round == 2 {
                self.gate_failed = true;
                self.worsen(ExitCode::Problems);
                return;
            }
            // A compile that runs no compiler, with none named or with --accept, makes no
            // change set and runs no unit again: the violated review fails it, and the next
            // compile plans those units with reason violated.
            // @lfy def/cli/compile.lfy:compile#compile:compile:d5f565b191777716fd8e847870720c599c23591f1d959c0cb67397a6770f7536
            // @lfy def/cli/compile.lfy:compile#compile:compile:0440d8388eff8352220c16c7b7cbcac23c06b46e2e0c8882f55b1629dc83a76a
            // @lfy def/cli/compile.lfy:compile#compile:compile:c51341f34278d51ccda0b1a75908b211f46b7db736525ed29c88351db922c346
            let command = match &self.compiler {
                Some(command) if !self.accept_only => command.clone(),
                _ => {
                    self.gate_failed = true;
                    self.worsen(ExitCode::Problems);
                    return;
                }
            };
            // The first plans the units whose markers answered for a violated review with
            // reason violated in their change sets, and they run again.
            // @lfy def/cli/compile.lfy:compile#compile:compile:1435376dfcd2c7ecb8f1966cb1ce291a33eaded750ea02d6c8b936baa000130c
            // @lfy def/cli/compile.lfy:compile#compile:compile:afe98c81a441acd8328abb1bd1e4139cb546203a7431be38e71303cf11efb5ea
            self.plan_violated(&violated);
            round = 2;
            self.run_units(&command);
        }
    }

    /// The units whose markers answered for a violated review of the global review planned
    /// with reason violated in their target's change set, at their place in plan order, their
    /// state planned; every other unit of the change set keeps its state. A target the compile
    /// works no change set for yet has one made new, holding those units; one of a target
    /// --target leaves out is not planned.
    // @lfy def/cli/compile.lfy:compile#compile:compile:1435376dfcd2c7ecb8f1966cb1ce291a33eaded750ea02d6c8b936baa000130c
    pub(crate) fn plan_violated(&mut self, stems: &[String]) {
        for stem in stems {
            if !self.planning.violated.contains(stem) {
                self.planning.violated.push(stem.clone());
            }
        }
        self.plan_again();
        let order: Vec<(String, String)> = self
            .plan
            .units
            .iter()
            .map(|unit| (target_of(&self.program.workspace, unit).to_string(), unit.stem.clone()))
            .collect();
        for index in 0..self.plan.units.len() {
            let (name, stem) = order[index].clone();
            if !stems.contains(&stem) || self.target.as_deref().is_some_and(|target| target != name) {
                continue;
            }
            // @lfy def/cli/compile.lfy:compile#compile:compile:1435376dfcd2c7ecb8f1966cb1ce291a33eaded750ea02d6c8b936baa000130c
            if !self.sets.contains_key(&name) {
                let Some(named) = self.program.workspace.targets.iter().find(|named| named.identifier == name).cloned()
                else {
                    continue;
                };
                let Some(set) = self.new_change_set(&named, Vec::new()) else { continue };
                self.sets.insert(name.clone(), set);
            }
            // A unit the global review planned is planned during the compile.
            // @lfy def/cli/compile.lfy:compile#compile:compile:9fb81fa1507e767993a4d11085badfdfe00d34fe56bfd3c332d91cc5d3215da5
            self.schedule.again.insert(index);
            let Some(set) = self.sets.get_mut(&name) else { continue };
            match set.units.iter().position(|held| held.stem == stem) {
                Some(place) => {
                    set.units[place].state = UnitState::Planned;
                    set.units[place].reason = Reason::Violated;
                }
                None => {
                    let at = set
                        .units
                        .iter()
                        .position(|held| order.iter().position(|(_, one)| *one == held.stem) > Some(index))
                        .unwrap_or(set.units.len());
                    set.units.insert(at, ChangeSetUnit::planned(&stem, Reason::Violated, "", ""));
                }
            }
            self.save(&name);
        }
        self.count_done();
    }

    /// The final gate on each change set, in the order of the targets, each after one
    /// finalGate progress line for its target; whether every one passed. One that fails has
    /// its problems follow its line, and no final gate runs after it.
    // @lfy def/cli/compile.lfy:compile#compile:compile:f2d95b937709d596c90877c72c560452b08ca12e639e6792b8be4919cd63a3f9
    // @lfy def/cli/compile.lfy:compile#compile:compile:10903918821d4be8e7337ec0cd731a8dcac0f86cf39d669061103d9aa1c4f974
    // @lfy def/cli/compile.lfy:compile#compile:compile:3d56bbbdda35f1bdb3f31f4a0bcb7a611efd511b697c7deed056bda76554bac2
    pub(crate) fn final_gates(&mut self) -> bool {
        let targets: Vec<Target> = self.program.workspace.targets.clone();
        for named in &targets {
            if !self.sets.contains_key(&named.identifier) {
                continue;
            }
            // @lfy def/cli/compile.lfy:compile#compile:compile:b6e79b3ca5fa492391881efa95a4994ac3f869e7623e0954b6788a6bc1a06e2a
            self.reporter.report(Step::FinalGate, None, &named.identifier);
            let problems = self.final_gate(named);
            // @lfy def/cli/compile.lfy:compile#compile:compile:10903918821d4be8e7337ec0cd731a8dcac0f86cf39d669061103d9aa1c4f974
            if problems.is_empty() {
                continue;
            }
            // It fails: its problems follow its line, no final gate after it, no global
            // review, and no merge runs, and every change set is kept. No unit runs while the
            // final gates do, so nothing comes between the line and its problems.
            // @lfy def/cli/compile.lfy:compile#compile:compile:3d56bbbdda35f1bdb3f31f4a0bcb7a611efd511b697c7deed056bda76554bac2
            self.reporter.problems(&problems);
            self.gate_failed = true;
            self.worsen(ExitCode::Problems);
            return false;
        }
        true
    }

    /// The problems of one change set's final gate: every stub line of a file of the target's
    /// output directory in the copy, then, when there is none, the failure of the target's
    /// build, test with stubs not allowed, lint, and format commands, run in the copy in that
    /// order, each only once the one before it succeeded and skipped when the target has no
    /// line for it; and every unit not reviewed, when a verifier is to review, or neither
    /// accepted nor reviewed otherwise.
    // @lfy def/cli/compile.lfy:compile#compile:compile:f1b10f02356ca91bbce69918d73b71540bc544c39e28653ce88393191e8e8ffe
    // @lfy def/cli/compile.lfy:compile#compile:compile:67a1f8d3c09f325ad74066024558e41cfb46dd0f03a0b40b1a053070d1333908
    // @lfy def/cli/compile.lfy:compile#compile:compile:af81d2fde0deb57f61f264471f2b5a36f8c25c4a05d0bdf88527db960a95075b
    // @lfy def/cli/compile.lfy:compile#compile:compile:5d082ae0ac17d428f19ee0894765ffab9b0227cf846ca57511c6451d474a7ff0
    // @lfy def/cli/compile.lfy:compile#compile:compile:14975084f7075420945bc623c20fc6a060a72225fd00c5d0589607233852712a
    // @lfy def/cli/compile.lfy:compile#compile:compile:041f6b776f5a24240eda0d930f6f59f73477128b53f77574b05efcf1ee65d4d0
    pub(crate) fn final_gate(&mut self, target: &Target) -> Vec<String> {
        let Some(set) = self.sets.get(&target.identifier).cloned() else { return Vec::new() };
        let copy = PathBuf::from(&set.directory);
        let mut problems = Vec::new();
        // Every stub line of a file of the output directory in the copy, every directory
        // named target, .git, or node_modules skipped, is one problem naming the file's path
        // relative to the copy and the line number.
        // @lfy def/cli/compile.lfy:compile#compile:compile:f1b10f02356ca91bbce69918d73b71540bc544c39e28653ce88393191e8e8ffe
        // @lfy def/cli/compile.lfy:compile#compile:compile:05a98f1380573aab179458037c363fd33d4604fb69940c131e203ae327ee9388
        if let Some(stub) = stub_line_of(&self.program.workspace, target) {
            for file in files_in(target, &copy) {
                for (at, line) in file.text.lines().enumerate() {
                    // @lfy def/cli/compile.lfy:compile#compile:compile:f1b10f02356ca91bbce69918d73b71540bc544c39e28653ce88393191e8e8ffe
                    if is_stub_line(&stub, line) {
                        problems.push(format!("{}:{}: is a stub line", file.path, at + 1));
                    }
                }
            }
        }
        // With no stub line, the build, test, lint, and format commands run in the copy, the
        // test with stubs not allowed, each only once the one before it succeeded.
        // @lfy def/cli/compile.lfy:compile#compile:compile:67a1f8d3c09f325ad74066024558e41cfb46dd0f03a0b40b1a053070d1333908
        if problems.is_empty() {
            let ran_in = copy.join(&target.output_directory);
            for operation in [Operation::Build, Operation::Test, Operation::Lint, Operation::Format] {
                // A command the target has no line for is skipped and counts as succeeded.
                // @lfy def/cli/compile.lfy:compile#compile:compile:af81d2fde0deb57f61f264471f2b5a36f8c25c4a05d0bdf88527db960a95075b
                if line_for(target, operation).is_none() {
                    continue;
                }
                // @lfy def/cli/compile.lfy:compile#compile:compile:625e4b5fb16259dc0adc0244165b4419cca124f091680c494fefd6ce6246e67b
                let Some(ran) = run_target_command(target, operation, &copy, Some(&self.running), false) else {
                    continue;
                };
                if ran.succeeded() {
                    continue;
                }
                // A command that could not run is said so, and the code is failure.
                // @lfy def/cli/compile.lfy:compile#compile:compile:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
                self.could_not_run(&ran);
                // One problem naming the operation and the exit code, followed by every line
                // of its output naming a location in a file of the output directory.
                // @lfy def/cli/compile.lfy:compile#compile:compile:5d082ae0ac17d428f19ee0894765ffab9b0227cf846ca57511c6451d474a7ff0
                let failure = format!("the {} command exited with {}", word_of_operation(operation), ran.code);
                problems.push(problem_with_lines(&failure, &lines_in_outputs(&ran, target, &ran_in, &copy)));
                break;
            }
        }
        // Every unit of the change set not reviewed, or neither accepted nor reviewed with no
        // verifier, is one problem naming it.
        // @lfy def/cli/compile.lfy:compile#compile:compile:14975084f7075420945bc623c20fc6a060a72225fd00c5d0589607233852712a
        // @lfy def/cli/compile.lfy:compile#compile:compile:041f6b776f5a24240eda0d930f6f59f73477128b53f77574b05efcf1ee65d4d0
        for unit in &set.units {
            let complete = match self.verifies() {
                true => unit.state == UnitState::Reviewed,
                false => unit.state.is_done(),
            };
            if !complete {
                problems.push(format!("{} is {}", unit.stem, unit.state.value()));
            }
        }
        problems
    }

    /// The global review of the compile, when a verifier is to review and either a change set
    /// is to be merged or the global ids differ from those the last global review answered
    /// for; the stems whose markers answered for a violated review, or none.
    ///
    /// Its directory is the root when the compile works no change set, that change set's copy
    /// when it works one, and the copy of the change set whose target is last among the
    /// targets when it works more, every other change set's changes written there first. Its
    /// source maps are those the map files of each change set's copy hold for its units, and
    /// the recorded ones of every other unit.
    // @lfy def/cli/compile.lfy:compile#compile:compile:0b09728c30bb0edb3d089358237c8d4a4b44e18393eb5824b8af1001750100e0
    // @lfy def/cli/compile.lfy:compile#compile:compile:67b4fcdb83c0d4fb7760fbaab79b47962fe177863768224d0870c81726f1cc08
    // @lfy def/cli/compile.lfy:compile#compile:compile:14716d93e16448c0464d1b36613a20d644d039670aec988719076afec4cd8213
    // @lfy def/cli/compile.lfy:compile#compile:compile:0e334c474ecc31799e36fab9eb93a1372745df07c394b425e5fa04afcb0f951d
    // @lfy def/cli/compile.lfy:compile#compile:compile:7c6f5e6de7536e86a256b2b8f3baa8bd24a1238786e81140e15d4950be8f911a
    pub(crate) fn review_globally(&mut self) -> Vec<String> {
        // @lfy def/cli/compile.lfy:compile#compile:compile:0b09728c30bb0edb3d089358237c8d4a4b44e18393eb5824b8af1001750100e0
        if !self.verifies() {
            return Vec::new();
        }
        let root = self.root();
        let requirements = GlobalReview::requirements_of(&self.program);
        let changed = GlobalReview::read(&root).is_none_or(|last| last.requirements != requirements);
        if self.sets.is_empty() && !changed {
            return Vec::new();
        }
        let names: Vec<String> = self
            .program
            .workspace
            .targets
            .iter()
            .map(|named| named.identifier.clone())
            .filter(|name| self.sets.contains_key(name))
            .collect();
        // @lfy def/cli/compile.lfy:compile#compile:compile:7c6f5e6de7536e86a256b2b8f3baa8bd24a1238786e81140e15d4950be8f911a
        // @lfy def/cli/compile.lfy:compile#compile:compile:14716d93e16448c0464d1b36613a20d644d039670aec988719076afec4cd8213
        let directory = match names.last() {
            None => root.clone(),
            Some(last) => PathBuf::from(&self.sets[last].directory),
        };
        // Every other change set's own files that differ from its base, were added, or were
        // removed are written into that copy, or removed there, first.
        // @lfy def/cli/compile.lfy:compile#compile:compile:0e334c474ecc31799e36fab9eb93a1372745df07c394b425e5fa04afcb0f951d
        for name in names.iter().rev().skip(1) {
            let set = &self.sets[name];
            let Some(named) = self.program.workspace.targets.iter().find(|named| &named.identifier == name) else {
                continue;
            };
            let changes = changes_of(&root, Path::new(&set.directory), named);
            let writes: Vec<(String, Option<Vec<u8>>, Option<u32>)> =
                changes.into_iter().map(|(path, change)| (path, change.text, change.mode)).collect();
            if let Err(path) = write_merges(&writes, &directory) {
                complain(self.style.err, Some(&path), "could not be written for the global review");
            }
        }
        // @lfy def/cli/compile.lfy:compile#compile:compile:67b4fcdb83c0d4fb7760fbaab79b47962fe177863768224d0870c81726f1cc08
        let maps = self.every_map();
        let Some(report) = self.global_review(&directory, &maps) else { return Vec::new() };
        let ids: BTreeSet<String> = report
            .reviews
            .iter()
            .filter(|review| review.status == ReviewStatus::Violated)
            .map(|review| review.id.clone())
            .collect();
        if ids.is_empty() {
            return Vec::new();
        }
        // @lfy def/cli/compile.lfy:compile#compile:compile:1435376dfcd2c7ecb8f1966cb1ce291a33eaded750ea02d6c8b936baa000130c
        self.stems_answering_with(&maps, |id| ids.contains(id))
    }

    /// Every source map of the program: those the map files of each change set's copy hold
    /// for its units, and the recorded ones of every other unit.
    // @lfy def/cli/compile.lfy:compile#compile:compile:67b4fcdb83c0d4fb7760fbaab79b47962fe177863768224d0870c81726f1cc08
    pub(crate) fn every_map(&self) -> Vec<SourceMap> {
        let root = self.root();
        let mut out = Vec::new();
        for unit in 0..self.plan.units.len() {
            let directory = match self.set_of(unit) {
                Some(set) if set.units.iter().any(|held| held.stem == self.plan.units[unit].stem) => {
                    PathBuf::from(&set.directory)
                }
                _ => root.clone(),
            };
            out.extend(self.maps_in(unit, &directory));
        }
        out
    }

    /// The root takes every change set of the compile: one merging progress line for each, in
    /// the order of the targets, and then every one of their own files that differs from its
    /// base, was added, or was removed merged into the root at once. No file of the root
    /// changes until every file has merged without a conflict; a file that cannot be has a
    /// problem naming it follow the merging line, and every change set is kept. A change set
    /// the root took leaves neither its file, its copy, nor its base.
    // @lfy def/cli/compile.lfy:compile#compile:compile:d65259b76766c2e72bd7a2d4077d7121611026a93aacf0a91ba6d86e97ddbe6d
    // @lfy def/cli/compile.lfy:compile#compile:compile:7faf028d9e62dd4a82a75683cb6aef188e05dc77f9dbd53210dd7cba92ba7277
    // @lfy def/cli/compile.lfy:compile#compile:compile:f62308cbdc193558b2d780fd52bf6661146cd7a2e58d88bb29026d348fb53412
    // @lfy def/cli/compile.lfy:compile#compile:compile:7ea6410a8673a0a0b1c2459596d167fac7037f12858c4c99845e67852a0efb8b
    // @lfy def/cli/compile.lfy:compile#compile:compile:620fa0faf525e60e5d31d15d5f9c42d2193013818b1cf267a45bff765761c588
    // @lfy def/cli/compile.lfy:compile#compile:compile:5d1e63160121bb44aeb91bd7d1f6eced00a1991447dd7d14e77ff2e4a4bf2f9a
    pub(crate) fn take_change_sets(&mut self) {
        if self.sets.is_empty() {
            return;
        }
        let root = self.root();
        let mut every: Vec<Changes> = Vec::new();
        let targets: Vec<Target> = self.program.workspace.targets.clone();
        for named in &targets {
            let Some(set) = self.sets.get(&named.identifier) else { continue };
            // @lfy def/cli/compile.lfy:compile#compile:compile:d65259b76766c2e72bd7a2d4077d7121611026a93aacf0a91ba6d86e97ddbe6d
            // @lfy def/cli/compile.lfy:compile#compile:compile:b6e79b3ca5fa492391881efa95a4994ac3f869e7623e0954b6788a6bc1a06e2a
            let directory = set.directory.clone();
            self.reporter.report(Step::Merging, None, &named.identifier);
            every.push(changes_of(&root, Path::new(&directory), named));
        }
        // @lfy def/cli/compile.lfy:compile#compile:compile:f62308cbdc193558b2d780fd52bf6661146cd7a2e58d88bb29026d348fb53412
        match merge_all(&every, &root) {
            Ok(_) => {
                // @lfy def/cli/compile.lfy:compile#compile:compile:5d1e63160121bb44aeb91bd7d1f6eced00a1991447dd7d14e77ff2e4a4bf2f9a
                let sets: Vec<ChangeSet> = self.sets.values().cloned().collect();
                // What each unit came to stays known for the finished line.
                for set in sets {
                    if let Err(error) = remove_change_set(&root, &set) {
                        complain(self.style.err, Some(&set.directory), &error.to_string());
                    }
                }
            }
            // @lfy def/cli/compile.lfy:compile#compile:compile:7ea6410a8673a0a0b1c2459596d167fac7037f12858c4c99845e67852a0efb8b
            Err(path) => {
                self.reporter.problems(&[format!("{path} could not be merged into the root without a conflict")]);
                self.gate_failed = true;
                self.worsen(ExitCode::Problems);
            }
        }
    }

    /// The last line: the count accepted, rejected, blocked, and failed, each a tally. Its
    /// glyph and name are success when nothing was rejected, blocked, or failed and no final
    /// gate, global review, or merge failed, and failure when something was or did. The
    /// planned units that are neither accepted nor reviewed follow on one line, indented,
    /// painted warning.
    // @lfy def/cli/compile.lfy:compile#compile:compile:4da6fc0ef0b92bdd309365b39295a5cc5de6e9d3296556c8da4e7fc7e1c11920
    // @lfy def/cli/compile.lfy:compile#compile:compile:1ece271551709a1351fbd18d13e9516ddea9d6fb7ad14955145eb9f320db5bf5
    // @lfy def/cli/compile.lfy:compile#compile:compile:4fb67ab23f1c2255a1177088bce0458b4c7fc72eee0b9f60cc2c47cdc0e85480
    pub(crate) fn finish(&mut self) {
        self.count_done();
        let planned = self.planned_units();
        // A unit counts as accepted when its state is accepted or reviewed at the end of the
        // compile, a unit kept from a resumed change set included.
        // @lfy def/cli/compile.lfy:compile#compile:compile:4fb67ab23f1c2255a1177088bce0458b4c7fc72eee0b9f60cc2c47cdc0e85480
        let accepted = planned.iter().filter(|&&unit| self.state_of(unit).is_done()).count();
        // Each count and its word, the four separated by a comma and a space.
        // @lfy def/cli/compile.lfy:compile#compile:compile:1ece271551709a1351fbd18d13e9516ddea9d6fb7ad14955145eb9f320db5bf5
        // @lfy def/cli/compile.lfy:compile#compile:compile:081257eb508c08a673bc5c569b3099485967e6b85b434c98d303c2de2d9958c5
        let counts: Counts = vec![
            (accepted, "accepted", Tone::Success),
            (self.rejected, "rejected", Tone::Failure),
            (self.blocked, "blocked", Tone::Warning),
            (self.failed, "failed", Tone::Failure),
        ];
        // @lfy def/cli/compile.lfy:compile#compile:compile:71142bfb2415229adff427e0618a8e7fed7785d96980ddf1f503a55a7a62af12
        // @lfy def/cli/compile.lfy:compile#compile:compile:3bd28015ddcd6d08a3df7365b38b07af984f8563916c03a19a03c140696c78ce
        let went_wrong = self.rejected > 0 || self.blocked > 0 || self.failed > 0 || self.gate_failed;
        let tone = if went_wrong { Tone::Failure } else { Tone::Success };
        // With --json the one object's message is the four counts alone.
        // @lfy def/cli/compile.lfy:compile#compile:compile:bbad3c69582995a56ff9152a6a483894f2d276f7b0d0b4805da999767796b37e
        if self.json {
            let message = counts_message(&counts, false);
            self.reporter.report(Step::Finished, None, &message);
            return;
        }
        self.reporter.report_counts(Step::Finished, None, &counts, Some(tone));
        // The planned units neither accepted nor reviewed follow, in plan order, on one line
        // indented four spaces and painted warning.
        // @lfy def/cli/compile.lfy:compile#compile:compile:284d09f3b4a49da7c8306079f44a6a97f2de5d712f9c7306fcf43a6d5fac53b9
        // @lfy def/cli/compile.lfy:compile#compile:compile:100d74724aaa583127f38518ebec30b1d925183714d421ec1f391ebe6c94d841
        let incomplete: Vec<String> =
            planned.iter().filter(|&&unit| !self.state_of(unit).is_done()).map(|&unit| self.stem(unit)).collect();
        if !incomplete.is_empty() {
            let line = format!("did not complete: {}", incomplete.join(", "));
            self.reporter.reason(&line);
        }
    }
}
