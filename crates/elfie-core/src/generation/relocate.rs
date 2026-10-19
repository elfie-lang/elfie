// @lfy def/generation/accept.lfy:relocate
//! Relocation: the code of entities whose declaration moved to another file, given to the
//! unit of that file by rewriting the markers that name it.

use super::compile_data::{REVIEWS, own_files, read_review_records, rederive_in, source_maps_under};
use super::*;

/// Directories never walked for outputs: a build's own output, a repository's data, and a
/// package manager's downloads.
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:4d0103d3f96ff9071e2501adbecf2688ff28d6f0b3a3450a4eaa87de84415d55
const SKIPPED: [&str; 3] = ["target", ".git", "node_modules"];

/// The markers, source maps, and review records of every entity whose code the outputs hold
/// under the name of a file that no longer declares it, rewritten to name the one file that
/// does.
///
/// The outputs are every file of each target's output directory under `directory`, or under
/// the workspace root when it is `None`. A marker naming an entity of a project file that
/// declares no such entity for the output's target is orphaned; the files that do declare
/// one are its candidates, narrowed, where markers of the same file and entity carry ids, to
/// those whose entity still answers for one of them. The orphaned markers of one file and
/// entity are rewritten together when exactly one candidate is left, and left as they are
/// otherwise. The maps of every unit code moved out of or into, or whose maps name a
/// rewritten output, are derived again as the outputs read once rewritten, and the review
/// records of the relocated entities move with their code. Nothing is written, and no
/// compiler, verifier, command of the target, or git is run.
// Decision: the definition passes the plan; a plan here does not own its workspace or the
// program lowered from it, so the program is an extra first parameter, as for `review`.
// @lfy def/generation/accept.lfy:relocate
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:33b7f9769323ab6b2fd84b9cf7d4d1eb7a6255ca9cc0b9f975464704b877be0d
pub fn relocate(program: &Program, plan: &Plan, directory: Option<&Path>) -> Relocated {
    let workspace = &program.workspace;
    let directory = directory.unwrap_or(workspace.root.as_path());
    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:b365192b3311058a64e4835640b7d8f4d411459fd0cebd5be19c821e4b3e6288
    let maps_read = source_maps_under(workspace, directory, None);

    // The outputs of each target, and the markers of each, rewritten where one candidate is
    // left.
    let mut relocations: Vec<Relocation> = Vec::new();
    let mut files: Vec<Vec<Output>> = Vec::new();
    let mut rewritten: Vec<Output> = Vec::new();
    // The ids each relocation's rewritten markers carry, by its index in `relocations`.
    let mut carried: Vec<Vec<String>> = Vec::new();
    for target in &workspace.targets {
        let outputs = outputs_under(directory, &target.output_directory);
        let (mut found, groups) = relocations_of(program, target, &outputs);
        // @lfy def/generation/accept.lfy:relocate#relocate:relocate:5fb8c262b31de69c1722a9c987d846610497e6d16ff3e10c58ab920056275481
        let relocated: Vec<Output> = outputs
            .iter()
            .filter_map(|output| {
                let text = rewritten_text(&output.text, &groups)?;
                Some(Output {
                    path: output.path.clone(),
                    text,
                })
            })
            .collect();
        // The files as relocated: each rewritten output as its text is now, every other file
        // as it is.
        // @lfy def/generation/accept.lfy:relocate#relocate:relocate:1be884fee5cf1d76d6ee16c247bb9752196fe8f49f22d8700b6789d33f8800fa
        let as_relocated: Vec<Output> = outputs
            .into_iter()
            .map(|output| {
                relocated
                    .iter()
                    .find(|changed| changed.path == output.path)
                    .cloned()
                    .unwrap_or(output)
            })
            .collect();
        carried.extend(found.iter().map(|(_, ids)| ids.clone()));
        relocations.extend(found.drain(..).map(|(relocation, _)| relocation));
        files.push(as_relocated);
        for output in relocated {
            if !rewritten.iter().any(|held| held.path == output.path) {
                rewritten.push(output);
            }
        }
    }
    rewritten.sort_by(|left, right| left.path.cmp(&right.path));

    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:ed503738db6fcf03f63af23719e2dac5524eb316ff0a2527f41665d52e24ba05
    if relocations.is_empty() {
        return Relocated {
            source_maps: maps_read,
            ..Relocated::default()
        };
    }

    let source_maps = maps_relocated(
        program,
        plan,
        &maps_read,
        &relocations,
        &carried,
        &rewritten,
        &files,
    );
    let reviews = reviews_relocated(program, plan, directory, &relocations);
    Relocated {
        relocations,
        outputs: rewritten,
        source_maps,
        reviews,
    }
}

/// Every file of an output directory under `directory`, recursively, skipping every
/// directory named as [`SKIPPED`] says, each read as text, with its path relative to the
/// root, in path order. A file that cannot be read as text is left out.
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:4d0103d3f96ff9071e2501adbecf2688ff28d6f0b3a3450a4eaa87de84415d55
fn outputs_under(directory: &Path, output_directory: &str) -> Vec<Output> {
    let mut paths: Vec<String> = Vec::new();
    walk(&directory.join(output_directory), output_directory, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(directory.join(&path)).ok()?;
            Some(Output { path, text })
        })
        .collect()
}

/// Every file at or under one directory, as a path relative to the root with `/` between
/// the segments, skipping the directories [`SKIPPED`] names.
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:4d0103d3f96ff9071e2501adbecf2688ff28d6f0b3a3450a4eaa87de84415d55
fn walk(directory: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = under_root(prefix, &name);
        let path = entry.path();
        if path.is_dir() {
            if !SKIPPED.contains(&name.as_str()) {
                walk(&path, &relative, out);
            }
        } else {
            out.push(relative);
        }
    }
}

/// The relocations of one target's outputs, each with the ids its rewritten markers carry,
/// in `Workspace::files` order of [`Relocation::into`] and then in the order the first
/// marker of each appears in the outputs; and, for each, the file and entity its markers
/// named and the file they name now.
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:1a059000fb6ab9058a68deed6e5262648945ef089d52434a0e87147e30c1372f
#[allow(clippy::type_complexity)]
fn relocations_of(
    program: &Program,
    target: &Target,
    outputs: &[Output],
) -> (Vec<(Relocation, Vec<String>)>, Vec<(String, String, String)>) {
    let workspace = &program.workspace;
    // Every orphaned marker, grouped by the file and entity it names, in the order the first
    // of each group appears; with every id a marker of the same file and entity carries.
    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:990b22675864bed9546b54d04431029d79dc65c6ec03bfc62e2b5067f4f1de50
    let mut groups: Vec<(String, String, usize)> = Vec::new();
    let mut ids: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for output in outputs {
        for marker in parse_markers(&output.text) {
            let Some(entity) = &marker.entity else { continue };
            let key = (marker.file.clone(), entity.clone());
            if let Some(id) = &marker.requirement {
                let held = ids.entry(key.clone()).or_default();
                if !held.contains(id) {
                    held.push(id.clone());
                }
            }
            if !is_orphaned(workspace, target, &marker.file, entity) {
                continue;
            }
            match groups
                .iter_mut()
                .find(|(file, name, _)| file == &key.0 && name == &key.1)
            {
                Some(group) => group.2 += 1,
                None => groups.push((key.0, key.1, 1)),
            }
        }
    }
    let mut found: Vec<(usize, Relocation, Vec<String>)> = Vec::new();
    for (origin, entity, count) in groups {
        let asked = ids
            .get(&(origin.clone(), entity.clone()))
            .cloned()
            .unwrap_or_default();
        let mut candidates = candidates_of(program, target, &origin, &entity);
        // Only a candidate that still answers for what the code answered for.
        // @lfy def/generation/accept.lfy:relocate#relocate:relocate:a519caccbc677cf75a549ad61efab1ac3308d321a0569fe1405c1b60e878a212
        if !asked.is_empty() {
            candidates.retain(|&(_, declared)| {
                let held = entity_ids(program, declared);
                asked.iter().any(|id| held.contains(id))
            });
        }
        // @lfy def/generation/accept.lfy:relocate#relocate:relocate:be73db54a2d75fbbad58de702fb271bc2d14eff7a087d844a030c8318ee2a2eb
        let [(file, _)] = candidates[..] else {
            continue;
        };
        found.push((
            file,
            Relocation {
                target: target.identifier.clone(),
                entity,
                origin,
                into: workspace.files[file].path.clone(),
                markers: count,
            },
            asked,
        ));
    }
    // Stable, so relocations into one file keep the order of their first markers.
    found.sort_by_key(|(file, _, _)| *file);
    let renames: Vec<(String, String, String)> = found
        .iter()
        .map(|(_, relocation, _)| {
            (
                relocation.origin.clone(),
                relocation.entity.clone(),
                relocation.into.clone(),
            )
        })
        .collect();
    (
        found
            .into_iter()
            .map(|(_, relocation, asked)| (relocation, asked))
            .collect(),
        renames,
    )
}

/// Whether a marker naming `entity` of `file` is orphaned for a target: the file is a file of
/// the workspace with no package, and declares no entity built for the target whose
/// identifier is the name, or for a name with a dot, the owner's identifier.
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:d571fb77bdde93f51e6308b17cf2d32fc4f3abd9a421a361b314697542ddd5f9
fn is_orphaned(workspace: &Workspace, target: &Target, file: &str, entity: &str) -> bool {
    let Some(declaring) = workspace.file(file) else {
        return false;
    };
    if declaring.package.is_some() {
        return false;
    }
    let owner = entity.split_once('.').map_or(entity, |(owner, _)| owner);
    let model = &workspace.model;
    !built_entities(model, declaring.source, target.declaration)
        .iter()
        .any(|&built| model.entities[built].identifier.as_deref() == Some(owner))
}

/// The other files of the workspace with no package that declare an entity built for the
/// target named as `entity` names one, or, for a name with a dot, an entity with the owner's
/// identifier holding a member of that name; each as its index into `Workspace::files` and
/// the entity the name names there.
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:235508d273bf7a0baa2e362b069972c22f8112165ad4c2446e0c3a312196c918
fn candidates_of(
    program: &Program,
    target: &Target,
    origin: &str,
    entity: &str,
) -> Vec<(usize, EntityId)> {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let (owner, member) = match entity.split_once('.') {
        Some((owner, member)) => (owner, Some(member)),
        None => (entity, None),
    };
    let mut out: Vec<(usize, EntityId)> = Vec::new();
    for (index, file) in workspace.files.iter().enumerate() {
        if file.package.is_some() || file.path == origin {
            continue;
        }
        let declared = built_entities(model, file.source, target.declaration)
            .into_iter()
            .filter(|&built| model.entities[built].identifier.as_deref() == Some(owner))
            .find_map(|built| match member {
                None => Some(built),
                Some(member) => members_of(model, built)
                    .into_iter()
                    .find(|&held| model.entities[held].identifier.as_deref() == Some(member)),
            });
        if let Some(declared) = declared {
            out.push((index, declared));
        }
    }
    out
}

/// The ids of the local criteria and tests of one entity and of its members, as its
/// lowered declaration holds them.
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:a519caccbc677cf75a549ad61efab1ac3308d321a0569fe1405c1b60e878a212
fn entity_ids(program: &Program, entity: EntityId) -> Vec<String> {
    let Some(node) = lowered_declaration(program, entity) else {
        return Vec::new();
    };
    let mut nodes = Vec::new();
    lowered_nodes(node, &mut nodes);
    nodes
        .iter()
        .flat_map(|node| {
            node.criteria
                .iter()
                .map(|criterion| criterion.id.clone())
                .chain(node.tests.iter().map(|test| test.id.clone()))
        })
        .collect()
}

/// The text of an output with the path of every marker naming a relocated file and entity
/// replaced by the file it names now, and every other character of each line, and every
/// other line, kept; `None` when it holds no such marker.
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:06c13663c823f02268ff24aa0f738e83e5f78bd0a30f52df199dc354a1ddeb64
fn rewritten_text(text: &str, renames: &[(String, String, String)]) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    for line in text.split_inclusive('\n') {
        let mut rest = line;
        while let Some(position) = rest.find(MARKER_PREFIX) {
            let start = position + MARKER_PREFIX.len();
            let (before, after) = rest.split_at(start);
            let length = after.find(char::is_whitespace).unwrap_or(after.len());
            let (token, tail) = after.split_at(length);
            out.push_str(before);
            let into = parse_marker(token, 1).and_then(|marker| {
                let entity = marker.entity.as_ref()?;
                renames
                    .iter()
                    .find(|(origin, name, _)| origin == &marker.file && name == entity)
                    .map(|(origin, _, into)| (origin.len(), into))
            });
            match into {
                Some((length, into)) => {
                    out.push_str(into);
                    out.push_str(&token[length..]);
                    changed = true;
                }
                None => out.push_str(token),
            }
            rest = tail;
        }
        out.push_str(rest);
    }
    changed.then_some(out)
}

/// The unit of a target whose file is at `path`, as an index into [`Plan::units`].
fn unit_at(program: &Program, plan: &Plan, target: &str, path: &str) -> Option<usize> {
    let workspace = &program.workspace;
    plan.units.iter().position(|unit| {
        workspace.targets[unit.target].identifier == target && workspace.files[unit.file].path == path
    })
}

/// The source maps read, with the maps of each unit a relocation moved code out of or into,
/// or one of whose maps names a rewritten output, derived again from the files as relocated;
/// each such unit's maps take the place of its first map read, and those of a unit no map
/// was read of come last.
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:8c8c0549bee9699f3d5acd1c42e3114dd32e49cd04eb2fb184f2340c04d80df3
fn maps_relocated(
    program: &Program,
    plan: &Plan,
    maps_read: &[SourceMap],
    relocations: &[Relocation],
    carried: &[Vec<String>],
    rewritten: &[Output],
    files: &[Vec<Output>],
) -> Vec<SourceMap> {
    let workspace = &program.workspace;
    // The units whose maps are derived again, by target and source path, in the order they
    // are first met.
    let mut affected: Vec<(String, String)> = Vec::new();
    let mut affect = |key: (String, String)| {
        if !affected.contains(&key) {
            affected.push(key);
        }
    };
    for map in maps_read {
        let key = (map.target.clone(), map.source.clone());
        let moved = relocations.iter().any(|relocation| {
            relocation.target == key.0 && (relocation.origin == key.1 || relocation.into == key.1)
        });
        if moved || rewritten.iter().any(|output| output.path == map.output) {
            affect(key);
        }
    }
    for relocation in relocations {
        affect((relocation.target.clone(), relocation.origin.clone()));
        affect((relocation.target.clone(), relocation.into.clone()));
    }

    let mut derived: Vec<((String, String), Vec<SourceMap>)> = Vec::new();
    for key in &affected {
        let (target, source) = key;
        let Some(target_index) = workspace
            .targets
            .iter()
            .position(|candidate| &candidate.identifier == target)
        else {
            continue;
        };
        let of_target = &files[target_index];
        let read: Vec<SourceMap> = maps_read
            .iter()
            .filter(|map| &map.target == target && &map.source == source)
            .cloned()
            .collect();
        let maps = match unit_at(program, plan, target, source) {
            Some(unit) => {
                let found = own_files(program, plan, unit, of_target);
                let moved_out = relocations
                    .iter()
                    .any(|relocation| &relocation.target == target && &relocation.origin == source);
                if found.is_empty() && (moved_out || read.is_empty()) {
                    // Code moved out of the unit and no file holds a marker of its own any more,
                    // or nothing of it was read and nothing holds one: it has no maps.
                    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:93c9f26b9163a6b3fda68ddf455863570eae244f4a471842aca21fa013a3f5de
                    Vec::new()
                } else if !read.is_empty() {
                    // Otherwise as rederive gives them, which keeps the maps as read where no
                    // file holds a marker of the unit's own.
                    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:84efefcf9edab1fb512d7edbadb3d52ba33e608fead07de81066b06907cd15f5
                    rederive_in(program, plan, unit, &read, of_target)
                } else {
                    // A unit code moved into and of which no map was read takes what the
                    // first map read of the unit the first relocation into it came from
                    // recorded, with no text.
                    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:8792744e69676cd1ef4f2b424f39a10564c50fd60fa42a3724583de37fa00f41
                    let template = relocations
                        .iter()
                        .find(|relocation| &relocation.target == target && &relocation.into == source)
                        .and_then(|relocation| {
                            maps_read.iter().find(|map| {
                                &map.target == target && map.source == relocation.origin
                            })
                        });
                    match template {
                        Some(template) => {
                            let template = SourceMap {
                                output: String::new(),
                                text: String::new(),
                                ..template.clone()
                            };
                            rederive_in(program, plan, unit, &[template], of_target)
                        }
                        None => Vec::new(),
                    }
                }
            }
            None => {
                // A file with no unit: its maps stand only while a file holds a marker
                // naming it.
                let named = of_target.iter().any(|output| {
                    parse_markers(&output.text)
                        .iter()
                        .any(|marker| &marker.file == source)
                });
                if named { read } else { Vec::new() }
            }
        };
        derived.push((key.clone(), maps));
    }

    // The ids a unit's maps answered for, and every id carried into it.
    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:75fa6b15d3932f256be72c37e82f0a66d665d357df32a5fa918c20083d3d798c
    for ((target, source), maps) in &mut derived {
        let into: Vec<usize> = (0..relocations.len())
            .filter(|&at| &relocations[at].target == target && &relocations[at].into == source)
            .collect();
        if into.is_empty() {
            continue;
        }
        let mut answered: Vec<String> = maps_read
            .iter()
            .filter(|map| &map.target == target && &map.source == source)
            .flat_map(|map| map.answered.iter().cloned())
            .collect();
        for at in into {
            answered.extend(carried[at].iter().cloned());
        }
        answered.sort();
        answered.dedup();
        for map in maps.iter_mut() {
            map.answered = answered.clone();
        }
    }

    let mut out: Vec<SourceMap> = Vec::new();
    let mut placed: Vec<(String, String)> = Vec::new();
    for map in maps_read {
        let key = (map.target.clone(), map.source.clone());
        match derived.iter().find(|(held, _)| held == &key) {
            None => out.push(map.clone()),
            Some((_, maps)) => {
                if !placed.contains(&key) {
                    out.extend(maps.iter().cloned());
                    placed.push(key);
                }
            }
        }
    }
    for (key, maps) in derived {
        if !placed.contains(&key) {
            out.extend(maps);
        }
    }
    out
}

/// The review records of each unit a record moved out of or into, in [`Plan::units`] order.
///
/// A unit's records are read from its review record file under `directory`, keeping every
/// record whatever its id. A record of a unit code was relocated out of moves when its id is
/// that of a local criterion or test, of the unit the code moved into, of a relocated entity
/// or of a member of one; it keeps its id, status, and hash.
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:fdd1bc51c38f9c62c44b1cbbd6f35402ebc120996898c5ea02ed62ade39fe519
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:dd567a2eb50a504594a9d8a80a5f42513601274f87909a8fd34ab55432c47b97
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:694389998ffd76aa31af05bd6c58accca7643be261e0e0c1c1f5eb06982e3d60
fn reviews_relocated(
    program: &Program,
    plan: &Plan,
    directory: &Path,
    relocations: &[Relocation],
) -> Vec<UnitReviews> {
    let workspace = &program.workspace;
    let stem_at = |path: &str| workspace.file(path).map(stem_of);
    // What each unit holds, by target and stem: the records it had, and those moved into it.
    let mut held: Vec<((String, String), Vec<ReviewRecord>, bool)> = Vec::new();
    let records_of = |held: &mut Vec<((String, String), Vec<ReviewRecord>, bool)>,
                      target: &str,
                      stem: &str|
     -> usize {
        let key = (target.to_string(), stem.to_string());
        if let Some(at) = held.iter().position(|(of, _, _)| of == &key) {
            return at;
        }
        let file = directory
            .join(REVIEWS)
            .join(target)
            .join(format!("{stem}.json"));
        held.push((key, read_review_records(&file), false));
        held.len() - 1
    };
    for relocation in relocations {
        let (Some(origin), Some(into)) = (stem_at(&relocation.origin), stem_at(&relocation.into))
        else {
            continue;
        };
        // The ids of the relocated entity and its members, as the file it moved into declares
        // them.
        let target = workspace
            .targets
            .iter()
            .find(|candidate| candidate.identifier == relocation.target);
        let Some(target) = target else { continue };
        let moving: Vec<String> = candidates_of(program, target, &relocation.origin, &relocation.entity)
            .into_iter()
            .filter(|&(file, _)| workspace.files[file].path == relocation.into)
            .flat_map(|(_, entity)| entity_ids(program, entity))
            .collect();
        let from = records_of(&mut held, &relocation.target, &origin);
        let to = records_of(&mut held, &relocation.target, &into);
        let (moved, kept): (Vec<ReviewRecord>, Vec<ReviewRecord>) = std::mem::take(&mut held[from].1)
            .into_iter()
            .partition(|record| moving.contains(&record.id));
        held[from].1 = kept;
        if moved.is_empty() {
            continue;
        }
        held[from].2 = true;
        held[to].2 = true;
        for record in moved {
            // Its own record is kept where both have the same id.
            if !held[to].1.iter().any(|own| own.id == record.id) {
                held[to].1.push(record);
            }
        }
    }
    let mut out: Vec<(usize, UnitReviews)> = held
        .into_iter()
        .filter(|(_, _, touched)| *touched)
        .map(|((target, stem), mut records, _)| {
            records.sort_by(|left, right| left.id.cmp(&right.id));
            let order = plan
                .units
                .iter()
                .position(|unit| {
                    unit.stem == stem && workspace.targets[unit.target].identifier == target
                })
                .unwrap_or(usize::MAX);
            (
                order,
                UnitReviews {
                    target,
                    stem,
                    records,
                },
            )
        })
        .collect();
    out.sort_by_key(|(order, _)| *order);
    out.into_iter().map(|(_, reviews)| reviews).collect()
}
