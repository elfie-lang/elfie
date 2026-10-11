// @lfy def/generation/main.lfy:accept
//! Whether outputs satisfy a request for one unit structurally.

use super::*;

/// Whether outputs satisfy a request for one unit structurally, the source maps they
/// earn, and the outputs with their markers normalized.
///
/// Rejected when there are no outputs, when an output shared with another unit of the
/// target — one a recorded map of that unit names — holds fewer of a region that unit's maps
/// record than those maps do, counting across the outputs written and every other file of
/// the output directory,
/// when an output's path is not under the target's
/// output directory or its extension is not among the target's, when a marker names an
/// entity that its file does not declare, a line
/// of the unit's file beyond its last, or — naming the unit's own file — a requirement id
/// that is neither a local criterion or test of the unit nor a global one, or when an entity
/// of the unit has no marker naming it or a line its declaration covers. A marker naming a
/// line that falls inside a declaration is rewritten to name that entity, since a name
/// survives edits and a line does not. A marker naming a file that is not in the program is
/// ignored and not recorded: it is a fixture or prose, not a claim about the program.
/// Otherwise accepted, with one source map per output, each marker carrying the end of the
/// region it begins.
/// Whether an output builds or its tests pass is not checked here; the guidance says how,
/// and the caller runs it.
///
/// `recorded` is the source maps of the other units of the unit's target, which say which
/// regions of a shared output belong to another unit, and `elsewhere` is every other file of
/// the target's output directory, as it is where the outputs are read. A file of `elsewhere`
/// is read for the markers it holds and for nothing else: it earns no source map, it is in
/// no [`Verdict::outputs`], and none of its markers is checked, so a region a unit moved out
/// of a shared file into a file of its own is still found where it went.
// Decision: the definition passes the request and the unit; a plan here does not own its
// workspace or the program lowered from it, so the program and the plan are extra first
// parameters and the unit is given as its index into the plan.
// @lfy def/generation/main.lfy:accept
// @lfy def/generation/main.lfy:accept#accept:accept:3735db25617f44c846b04cc15709baf47cbf8bfaeecebcf0e6adb99a577d2af4
pub fn accept(
    program: &Program,
    plan: &Plan,
    request: &Request,
    unit: usize,
    outputs: &[Output],
    recorded: &[SourceMap],
    elsewhere: &[Output],
) -> Verdict {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let planned = &plan.units[unit];
    let target = &workspace.targets[planned.target];
    let file = &workspace.files[planned.file];
    let mut problems: Vec<String> = Vec::new();

    if outputs.is_empty() {
        // @lfy def/generation/main.lfy:accept#accept:accept:4a48dc9bd184c822a38d627cf303e55c050d877bac33bf8d5f8794db459af17d
        problems.push(format!("no output was produced for {}", file.path));
    }

    let named = named_entities(model, file.source, &planned.entities);
    // Every requirement id a marker naming this unit's own file may spell: a local criterion
    // or test of this unit, or a global one of the request. An output is often shared with
    // another unit, which writes its own markers and its own ids into it, so only a marker
    // naming this unit's file is a claim this unit makes and only its id is checked here.
    // @lfy def/generation/main.lfy:accept
    let global_ids: Vec<&str> = request.globals.iter().map(Requirement::id).collect();
    let local_ids: Vec<String> = planned
        .requirements(program)
        .iter()
        .map(|requirement| requirement.id().to_string())
        .collect();
    // The files whose markers are this unit's claims: its own, and the file of each entity
    // moved into it, since a moved entity keeps the marker of the file that declares it.
    // @lfy def/generation/main.lfy:accept
    let mut claimed: Vec<&str> = vec![&file.path];
    for moved in plan.moves.iter().filter(|moved| moved.into == unit) {
        let path = &workspace.files[plan.units[moved.origin].file].path;
        if !claimed.contains(&path.as_str()) {
            claimed.push(path);
        }
    }

    // Each marker as the compiler wrote it, beside the one the model resolved it to: the
    // first says where the marker sits in the text, the second is what is recorded.
    // @lfy def/generation/main.lfy:accept
    let mut normalized: Vec<Vec<(Marker, Marker)>> = Vec::new();
    for output in outputs {
        if !under(&output.path, &target.output_directory) {
            // @lfy def/generation/main.lfy:accept#accept:accept:324f6bd2c925ffa32b5feee5d17d3195c149deb99f00247918a0ee6d58d1b870
            problems.push(format!(
                "the output {} is not under the output directory {} of the target {}",
                output.path, target.output_directory, target.identifier
            ));
        }
        // @lfy def/generation/main.lfy:accept#accept:accept:e25297fa468e4f2b18f120993ee14b756d7715661e928e3e161f9aba05aad304
        if !extension_of(&output.path).is_some_and(|extension| {
            target
                .extensions
                .iter()
                .any(|allowed| allowed == extension)
        }) {
            problems.push(format!(
                "the output {} has no extension the target {} allows: {}",
                output.path,
                target.identifier,
                target.extensions.join(", ")
            ));
        }
        let mut markers = Vec::new();
        // [`parse_markers`] derives each marker's end as the line before the output line
        // of the next marker in the same output, or the last line of the output for the
        // last one, so the regions of an output partition it from its first marker on.
        // @lfy def/generation/main.lfy:accept
        for marker in parse_markers(&output.text) {
            // A marker naming any other file — another file of the program, whose unit owns
            // the ids it spells, or a file outside the program, which is a fixture or prose
            // claiming nothing about it — spells no id this unit answers for, so none of
            // those is checked.
            // @lfy def/generation/main.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
            if let Some(requirement) = &marker.requirement
                && claimed.contains(&marker.file.as_str())
                && !local_ids.iter().any(|id| id == requirement)
                && !global_ids.iter().any(|&id| id == requirement)
            {
                problems.push(format!(
                    "{}:{}: the marker answers for {requirement}, which is no local criterion or \
                     test of {} and no global one",
                    output.path, marker.output_line, file.path
                ));
            }
            // @lfy def/generation/main.lfy:accept
            if let Some(resolved) = resolve_marker(
                workspace,
                file,
                &named,
                &marker,
                &claimed,
                output,
                &mut problems,
            ) {
                markers.push((marker, resolved));
            }
        }
        normalized.push(markers);
    }

    // An output is shared when a recorded map of another unit of the target names it: one
    // output often holds the code of several definition files, as a crate's root must. What
    // a unit may not do is take away what another unit recorded there, so each region of that
    // map which is not this unit's own must still be among the target's files as often as that
    // unit's maps record it — compared by the file, the entity, and the requirement it names
    // rather than by line, since every line moves when the code above it changes, and counted
    // across the outputs written and every other file of the output directory rather than in
    // the one output, since splitting a shared module moves a co-owner's regions into files of
    // their own. A region naming a file this unit claims is this unit's to write or to drop,
    // and a co-owner's map holds those only because a map records every marker of its output,
    // not because that unit put them there. A co-owner compiled in the same batch is writing
    // its own regions again in this very run, so its map says nothing about what this output
    // must still hold.
    // @lfy def/generation/main.lfy:accept#accept:accept:14f27c5c2a72fd5634bfa39ff79904e5261edd57a78c9b62e1afdfe63c7a7385
    // @lfy def/generation/main.lfy:accept#accept:accept:72d604363730e1a186d6ae456843c97ae6a857f121cf28d54e4bb331119e4af4
    let batched: Vec<&str> = request
        .batch
        .units
        .iter()
        .map(|&batched| workspace.files[plan.units[batched].file].path.as_str())
        .collect();
    // Every marker the target's files hold now: the resolved markers of the outputs this call
    // writes, and the markers of every other file of the output directory the caller read,
    // which are read for this count alone.
    // @lfy def/generation/main.lfy:accept#accept:accept:3735db25617f44c846b04cc15709baf47cbf8bfaeecebcf0e6adb99a577d2af4
    let mut held: Vec<Marker> = normalized
        .iter()
        .flatten()
        .map(|(_, kept)| kept.clone())
        .collect();
    for output in elsewhere {
        if outputs.iter().any(|written| written.path == output.path) {
            continue;
        }
        held.extend(parse_markers(&output.text));
    }
    for output in outputs {
        for map in recorded {
            if map.output != output.path
                || map.source == file.path
                || batched.contains(&map.source.as_str())
            {
                continue;
            }
            for marker in &map.markers {
                if is_own_marker(workspace, plan, unit, marker) {
                    continue;
                }
                let before = recorded
                    .iter()
                    .filter(|other| other.source == map.source)
                    .flat_map(|other| other.markers.iter())
                    .filter(|other| same_region(other, marker))
                    .count();
                let now = held
                    .iter()
                    .filter(|other| same_region(other, marker))
                    .count();
                if now < before {
                    problems.push(format!(
                        "the output {} is shared with {}, whose region {} is in {now} of this \
                         target's files and in {before} of its recorded maps",
                        output.path,
                        map.source,
                        region_of(marker)
                    ));
                }
            }
        }
    }

    // @lfy def/generation/main.lfy:accept#accept:accept:2a90cbf7112a647235b9f1c81167c5d44301cbc8369b5d4cc2973be6102fea26
    for &entity in &planned.entities {
        let Some((first, last)) = declaration_lines(model, entity) else {
            continue;
        };
        // An entity moved into the unit is marked with the file that declares it, not with
        // the unit's own file. @lfy def/generation/main.lfy:accept
        let declared = declaring_path(model, entity).unwrap_or(&file.path);
        let covered = normalized
            .iter()
            .flatten()
            .any(|(_, marker)| marker.file == *declared && (first..=last).contains(&marker.line));
        if !covered {
            problems.push(format!(
                "the entity {} of {declared} (lines {first} to {last}) has no marker in any output",
                entity_name(model, entity),
            ));
        }
    }

    if !problems.is_empty() {
        // @lfy def/generation/main.lfy:accept#accept:accept:03e40c9f3bc4db3e262e3c2c66d09c2f2d3d0833ad56ce201f1fafc09fa47f10
        return Verdict {
            accepted: false,
            problems,
            source_maps: Vec::new(),
            outputs: Vec::new(),
        };
    }

    // Nothing of a source map comes from the compiler's text except the names it spelled:
    // every hash, every signature, and the time are taken here.
    // @lfy def/generation/main.lfy:accept#accept:accept:1f423d0a0d06e5fc1e0ddf109f2e402b9a84537a80a91e6e4e459fb24ac288dc
    // @lfy def/generation/main.lfy:accept#accept:accept:90bf10cc4db0c256646cd0f1ba8c45bda247489103c46e1ce26289709829903a
    let hash = source_hash(&planned.text);
    // @lfy def/generation/main.lfy:accept#accept:accept:666051c035bda35cafa77995ca9f05915cb5d6f3d9dc74b172f9187d403c1a15
    let requirements = requirements_hash(program, planned);
    // The ids the hash is of, recorded beside it so a later run can say which of them the
    // unit no longer has.
    // @lfy def/generation/main.lfy:accept#accept:accept:0bcfc0b4637d084c09be1b3c407696d0b16f9c7f082c7f0d91de9ea2950d2902
    let answered = answered_ids(program, planned);
    // @lfy def/generation/main.lfy:accept#accept:accept:4a6c75702e77d3fab75aa1d127f35eaad993c997ed4cd0dd14f01162c019bc21
    let signature = interface_signature(workspace, planned);
    // @lfy def/generation/main.lfy:accept#accept:accept:d9879de703d6314164058495241caf82738d58c8f39595b07a19ace78cdbd758
    let dependencies: BTreeMap<String, String> = planned
        .dependencies
        .iter()
        .map(|&dependency| {
            let dependency = &plan.units[dependency];
            (
                workspace.files[dependency.file].path.clone(),
                interface_signature(workspace, dependency),
            )
        })
        .collect();
    // @lfy def/generation/main.lfy:accept#accept:accept:95161566ef61156ea6f857f212fb510a3edf6221a4238a87b45595f60b6466bb
    let generated = now_rfc3339();
    let mut source_maps = Vec::new();
    let mut written = Vec::new();
    // @lfy def/generation/main.lfy:accept#accept:accept:30a65355d64f16689cede881188462ef9f94082f48857623e59af57399ad7503
    for (output, markers) in outputs.iter().zip(&normalized) {
        // The output as it is written back is what the text hash is taken over, and of it only
        // the regions of this unit's own markers, stripped the way a review strips a region:
        // what another unit wrote in the same file, or added under one of its own markers, is
        // no part of what this unit's acceptance and review saw.
        let normalized_output = rewrite_markers(output, markers);
        let own = own_region_text(
            workspace,
            plan,
            unit,
            &target.marker_comment,
            &normalized_output.text,
            &markers
                .iter()
                .map(|(_, kept)| kept.clone())
                .collect::<Vec<_>>(),
        );
        source_maps.push(SourceMap {
            // @lfy def/generation/main.lfy:accept#accept:accept:8d2a3aa698445970d859538af521c42b878c576deae6eac97af3a97c5c7fd25f
            target: target.identifier.clone(),
            // @lfy def/generation/main.lfy:accept#accept:accept:ea45d8dd2770ae8698006fbdd6aa54ebb0af32fabeb5d9edf8a1a432812d7d7c
            output: output.path.clone(),
            // @lfy def/generation/main.lfy:accept#accept:accept:b27edcf9cd5b896cb993b81cba1f8faa47d79995f135c37e63d5e9ab648eed77
            source: file.path.clone(),
            hash: hash.clone(),
            requirements: requirements.clone(),
            answered: answered.clone(),
            signature: signature.clone(),
            // @lfy def/generation/main.lfy:accept#accept:accept:a31f0ba80f83a7d71e25224d7fa0612b14b3e59805cfb780c1293591243c7b01
            text: source_hash(&own),
            dependencies: dependencies.clone(),
            generated: generated.clone(),
            // @lfy def/generation/main.lfy:accept#accept:accept:e4b7f22f09d4b3bbbc78c4edf8763f1f23ff76f5f4e5c858304e8315e8fe30aa
            markers: markers.iter().map(|(_, marker)| marker.clone()).collect(),
        });
        written.push(normalized_output);
    }
    // Nothing here runs a command of the target: whether an output builds or its tests pass
    // is the caller's to run, as the guidance says.
    // @lfy def/generation/main.lfy:accept#accept:accept:9bde11d25fcf12e8c1a113f8be1c479d68185569c8170c39b3348772bf613541
    // @lfy def/generation/main.lfy:accept#accept:accept:03e40c9f3bc4db3e262e3c2c66d09c2f2d3d0833ad56ce201f1fafc09fa47f10
    Verdict {
        accepted: true,
        problems: Vec::new(),
        source_maps,
        outputs: written,
    }
}

/// Whether two markers name the same region: the same file, the same entity, and the same
/// requirement. Lines are left out on purpose — a region keeps its identity when the code
/// above it grows — and so is the output line, which is where the region sits rather than
/// what it is.
// @lfy def/generation/main.lfy:accept
fn same_region(one: &Marker, other: &Marker) -> bool {
    one.file == other.file && one.entity == other.entity && one.requirement == other.requirement
}

/// A region named the way a marker spells it, for a problem to quote: the file, the entity
/// or the line, and the requirement when it answers for one.
// @lfy def/generation/main.lfy:accept
fn region_of(marker: &Marker) -> String {
    let place = match &marker.entity {
        Some(entity) => entity.clone(),
        None => marker.line.to_string(),
    };
    match &marker.requirement {
        Some(requirement) => format!("{}:{place}#{requirement}", marker.file),
        None => format!("{}:{place}", marker.file),
    }
}

/// An output with every marker that was rewritten spelled as its name, so that the markers
/// kept are names and the lines recorded beside them are the model's.
///
/// Each is given as the compiler wrote it beside what it resolved to, and is found in the
/// text by the first: a spelling rebuilt from the line would be the model's line, which is
/// not what the compiler wrote and need not be found there at all.
// @lfy def/generation/main.lfy:accept
fn rewrite_markers(output: &Output, markers: &[(Marker, Marker)]) -> Output {
    let rewritten: Vec<&(Marker, Marker)> = markers
        .iter()
        .filter(|(_, marker)| marker.entity.is_some() && marker.column.is_none())
        .collect();
    if rewritten.is_empty() {
        return output.clone();
    }
    let mut lines: Vec<String> = output.text.lines().map(str::to_string).collect();
    let mut changed = false;
    for (as_written, marker) in rewritten {
        let Some(line) = lines.get_mut(marker.output_line.wrapping_sub(1)) else {
            continue;
        };
        let written = marker.spelling();
        let spelling = as_written.spelling();
        if spelling != written && line.contains(&spelling) {
            *line = line.replacen(&spelling, &written, 1);
            changed = true;
        }
    }
    if !changed {
        return output.clone();
    }
    let mut text = lines.join("\n");
    if output.text.ends_with('\n') {
        text.push('\n');
    }
    Output {
        path: output.path.clone(),
        text,
    }
}
