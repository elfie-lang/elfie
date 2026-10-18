// @lfy def/generation/main.lfy:accept
//! Whether outputs satisfy a request for one unit structurally.

use super::*;

/// Whether outputs satisfy a request for one unit structurally, the source maps they
/// earn, the outputs with their markers normalized, and the edits they make to later units.
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
///
/// A stub line left in a region of the unit's own is rejected, since a stub is for a later
/// unit's code. `before` is every file of the output directory as it was before the run; when
/// it is given, every line the run changed outside the outputs is judged: it must fall in the
/// region of a later unit of [`Reach::units`], leave that region's markers as they were, and
/// be a stub, or a fix within the cap of [`Reach`] whose owner `touches` gives a note for.
/// Each such region gives one [`ForeignEdit`]. `touches` is the note the run gave through
/// `elfie_touch` for each unit it edited, by stem.
// Decision: the definition passes the request and the unit; a plan here does not own its
// workspace or the program lowered from it, so the program and the plan are extra first
// parameters and the unit is given as its index into the plan.
// Decision: the touched-fix path needs nothing of its own here: a touched unit's outputs are
// accepted as they are, given as they are as before, so no line changed and the fixes become
// the unit's own code, which the text hash of its source maps then covers.
// @lfy def/generation/main.lfy:accept
// @lfy def/generation/main.lfy:accept#accept:accept:f294daed3a09adf31c632ee350bb60d1852500e5b36371b68343a7732c173749
#[allow(clippy::too_many_arguments)]
pub fn accept(
    program: &Program,
    plan: &Plan,
    request: &Request,
    unit: usize,
    outputs: &[Output],
    recorded: &[SourceMap],
    elsewhere: &[Output],
    before: Option<&[Output]>,
    touches: &BTreeMap<String, String>,
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
    // not because that unit put them there.
    // @lfy def/generation/main.lfy:accept#accept:accept:1aaaacdd8811d51407ed234af63ea8d8998b836c844531b97748fdce1bbb526d
    // @lfy def/generation/main.lfy:accept#accept:accept:153c7d11fef9fc4164bcdb50d754e6d780760ab490f4f9de86fc5aeb6eaa7b70
    // Every marker the target's files hold now: the resolved markers of the outputs this call
    // writes, and the markers of every other file of the output directory the caller read,
    // which are read for this count alone.
    // @lfy def/generation/main.lfy:accept#accept:accept:f294daed3a09adf31c632ee350bb60d1852500e5b36371b68343a7732c173749
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
            if map.output != output.path || map.source == file.path {
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

    // A stub is written only in a later unit's code, so a stub line left in a region of this
    // unit's own, whichever unit it names, is code still to be written.
    // @lfy def/generation/main.lfy:accept#accept:accept:851d0e32fd8478618d009c3df456da99f2bb0f1983e8e867a9c3e184ece945c9
    let stub_line = stub_line_of(model, target);
    if let Some(stub_line) = &stub_line {
        for (output, markers) in outputs.iter().zip(&normalized) {
            let lines: Vec<&str> = output.text.lines().collect();
            for (_, marker) in markers {
                if !is_own_marker(workspace, plan, unit, marker) {
                    continue;
                }
                let last = marker.end.min(lines.len());
                let first = marker.output_line.saturating_sub(1).min(last);
                for (offset, line) in lines[first..last].iter().enumerate() {
                    if is_any_stub_line(line, stub_line) {
                        problems.push(format!(
                            "{}:{}: a stub is left in the unit's code",
                            output.path,
                            first + offset + 1
                        ));
                    }
                }
            }
        }
    }

    // What the run changed outside its outputs, judged only when what the files held before
    // it is known.
    // @lfy def/generation/main.lfy:accept#accept:accept:b2a0c24659e730b5b7223cdf8ed1537a82470cbd637d779824236f5a354d9481
    let foreign_edits = match before {
        Some(before) => foreign_edits_of(
            program,
            plan,
            request,
            unit,
            outputs,
            elsewhere,
            before,
            touches,
            stub_line.as_deref(),
            &mut problems,
        ),
        None => Vec::new(),
    };

    if !problems.is_empty() {
        // @lfy def/generation/main.lfy:accept#accept:accept:03e40c9f3bc4db3e262e3c2c66d09c2f2d3d0833ad56ce201f1fafc09fa47f10
        return Verdict {
            accepted: false,
            problems,
            source_maps: Vec::new(),
            outputs: Vec::new(),
            foreign_edits: Vec::new(),
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
        foreign_edits,
    }
}

/// Whether a line, with the white space around it removed, reads the target's stub line with
/// `{{unit}}` as any stem and `{{reason}}` as any text: a stub line naming any unit.
// @lfy def/generation/main.lfy:accept#accept:accept:5e0b06a7232782f41a1259999162148cc42567b3738ee363433be4f33d64c65f
pub(super) fn is_any_stub_line(line: &str, stub_line: &str) -> bool {
    let template = stub_line.trim().replace("{{reason}}", "{{unit}}");
    let pieces: Vec<&str> = template.split("{{unit}}").collect();
    let line = line.trim();
    let (first, last) = (pieces[0], pieces[pieces.len() - 1]);
    if pieces.len() == 1 {
        return line == first;
    }
    if line.len() < first.len() + last.len() || !line.starts_with(first) || !line.ends_with(last) {
        return false;
    }
    // What lies between the fixed head and tail holds every middle piece, in order.
    let mut rest = &line[first.len()..line.len() - last.len()];
    for piece in &pieces[1..pieces.len() - 1] {
        match rest.find(piece) {
            Some(at) => rest = &rest[at + piece.len()..],
            None => return false,
        }
    }
    true
}

/// One region a run changed in a file that is none of its outputs: the marker that began it
/// before the run and the one that begins it now, where there is one, and the lines of each
/// side the diff changed, counted from 1. The two sides are paired by what their markers
/// name and by their order among the markers naming the same, which is the key.
// @lfy def/generation/main.lfy:accept#accept:accept:685361d7b868af843078c8f7efbabf1fcb62d73e6200f3553643c9b0133cb177
struct ChangedRegion {
    before: Option<Marker>,
    after: Option<Marker>,
    key: (String, usize),
    removed: Vec<usize>,
    added: Vec<usize>,
}

/// Every edit the run made inside a region of another unit, in files that are none of its
/// outputs, with a problem pushed for every changed line no edit may make.
///
/// The changed lines of a path are those a line diff removes from its text in `before` or
/// adds in its text in `elsewhere`, a path missing on either side read as empty. A line in
/// an output of `outputs` is the unit's own code, or, in a shared output, held only to the
/// marker counts, so no output is read here.
// @lfy def/generation/main.lfy:accept#accept:accept:0a1470345711d81632a94e78c934b73d81c67b8564e43fcffb6a91c40b535f73
// @lfy def/generation/main.lfy:accept#accept:accept:d2be4f6d1c48e93b0ad1958a7726a39791c4100c5d182cfc12e2b6389068f662
// @lfy def/generation/main.lfy:accept#accept:accept:ba9997e7c16a946b9bbb9bd3ea9e82580f4a5adf41fa4e4ac41bda4daadd677d
// @lfy def/generation/main.lfy:accept#accept:accept:8f5126f13dcf7841dfe05128e6dc1bc0a1d53614f5dc40393e3ba6a2195436de
#[allow(clippy::too_many_arguments)]
fn foreign_edits_of(
    program: &Program,
    plan: &Plan,
    request: &Request,
    unit: usize,
    outputs: &[Output],
    elsewhere: &[Output],
    before: &[Output],
    touches: &BTreeMap<String, String>,
    stub_line: Option<&str>,
    problems: &mut Vec<String>,
) -> Vec<ForeignEdit> {
    let workspace = &program.workspace;
    let planned = &plan.units[unit];
    let text_of = |files: &[Output], path: &str| -> String {
        files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.text.clone())
            .unwrap_or_default()
    };
    let mut paths: Vec<&str> = before
        .iter()
        .chain(elsewhere)
        .map(|file| file.path.as_str())
        .filter(|path| !outputs.iter().any(|output| output.path == *path))
        .collect();
    paths.sort();
    paths.dedup();

    let mut edits: Vec<ForeignEdit> = Vec::new();
    for path in paths {
        let old = text_of(before, path);
        let new = text_of(elsewhere, path);
        let old_lines: Vec<&str> = old.lines().collect();
        let new_lines: Vec<&str> = new.lines().collect();
        let (removed, added) = line_diff(&old_lines, &new_lines);
        if removed.is_empty() && added.is_empty() {
            continue;
        }
        let old_regions = program_regions(workspace, &old);
        let new_regions = program_regions(workspace, &new);
        // Each changed line in the region that held it before, for a removed line, or that
        // holds it now, for an added one; regions on the two sides are paired by what their
        // markers name and by their order among the markers naming the same.
        let mut regions: Vec<ChangedRegion> = Vec::new();
        let mut loose: Vec<usize> = Vec::new();
        let changed = removed
            .iter()
            .map(|&line| (false, line))
            .chain(added.iter().map(|&line| (true, line)));
        for (is_added, line) in changed {
            let side = if is_added { &new_regions } else { &old_regions };
            let Some((ordinal, marker)) = region_holding(side, line) else {
                loose.push(line);
                continue;
            };
            let key = (region_of(marker), ordinal);
            let index = match regions.iter().position(|region| region.key == key) {
                Some(index) => index,
                None => {
                    regions.push(ChangedRegion {
                        before: nth_like(&old_regions, marker, ordinal),
                        after: nth_like(&new_regions, marker, ordinal),
                        key,
                        removed: Vec::new(),
                        added: Vec::new(),
                    });
                    regions.len() - 1
                }
            };
            if is_added {
                regions[index].added.push(line);
            } else {
                regions[index].removed.push(line);
            }
        }

        // A changed line in no region of a file of the program is in no unit's code.
        // @lfy def/generation/main.lfy:accept#accept:accept:ce15bdbc6104d9c96f45ecd76aa1c3deb9e2e47c5bb449710162aefd10e3aff2
        if let Some(&line) = loose.first() {
            problems.push(format!(
                "{path}:{line}: the line is in no unit's code, so the run may not change it"
            ));
        }
        regions.sort_by_key(|region| {
            region
                .before
                .as_ref()
                .or(region.after.as_ref())
                .map_or(0, |marker| marker.output_line)
        });
        for region in regions {
            let Some(marker) = region.before.as_ref().or(region.after.as_ref()) else {
                continue;
            };
            let first_line = region
                .added
                .first()
                .or(region.removed.first())
                .copied()
                .unwrap_or(marker.output_line);
            // @lfy def/generation/main.lfy:accept#accept:accept:68cbc9770e3159775eb89fd93f184936bf7ac960196d0cf2094957f125ab4ecc
            let Some(owner) = owner_of(workspace, plan, planned.target, marker) else {
                // @lfy def/generation/main.lfy:accept#accept:accept:ce15bdbc6104d9c96f45ecd76aa1c3deb9e2e47c5bb449710162aefd10e3aff2
                problems.push(format!(
                    "{path}:{first_line}: the line is in no unit's code, so the run may not change \
                     it"
                ));
                continue;
            };
            // The unit's own code, wherever it is.
            // @lfy def/generation/main.lfy:accept#accept:accept:d2be4f6d1c48e93b0ad1958a7726a39791c4100c5d182cfc12e2b6389068f662
            if owner == unit {
                continue;
            }
            let owner_stem = &plan.units[owner].stem;
            // @lfy def/generation/main.lfy:accept#accept:accept:53c90dd1d996650fcb1634a0be592968e6c75780aa224f1f9d8b2a2c55d7bf8d
            if !request.reach.units.contains(&owner) {
                problems.push(format!(
                    "{path}:{first_line}: the line is in the code of {owner_stem}, which is not a \
                     later unit, and only the code of a later unit is edited"
                ));
                continue;
            }
            // @lfy def/generation/main.lfy:accept#accept:accept:9003974dcb53621de6101c21aa4bf533f452ad27b09cbc1480aacfe7cccae1ea
            let touches_marker = match (&region.before, &region.after) {
                (Some(old), Some(new)) => {
                    region.removed.contains(&old.output_line)
                        || region.added.contains(&new.output_line)
                }
                _ => true,
            } || region
                .removed
                .iter()
                .any(|&line| !parse_markers(old_lines[line - 1]).is_empty())
                || region
                    .added
                    .iter()
                    .any(|&line| !parse_markers(new_lines[line - 1]).is_empty());
            if touches_marker {
                problems.push(format!(
                    "{path}: the run changed the markers of the region {} of {owner_stem}, and the \
                     markers of another unit's region stay as they were",
                    region_of(marker)
                ));
                continue;
            }
            let (Some(old_marker), Some(new_marker)) = (&region.before, &region.after) else {
                continue;
            };
            let lines_of = |lines: &[&str], marker: &Marker| -> Vec<String> {
                let last = marker.end.min(lines.len());
                let first = marker.output_line.saturating_sub(1).min(last);
                lines[first..last].iter().map(|line| line.to_string()).collect()
            };
            // A stub adds only stub lines naming this unit; anything else is a fix.
            // @lfy def/generation/main.lfy:accept#accept:accept:8a1e611687ca02dd6ca89d5c5933e46879b0cbd6ee2ef85fc9e53c8848559b6d
            // @lfy def/generation/main.lfy:accept#accept:accept:8e628203f0c8221c728cebe1ce188a0dd58163d7974231ca4498cd36c2a91296
            let reasons: Vec<Option<String>> = region
                .added
                .iter()
                .map(|&line| {
                    stub_line.and_then(|stub_line| {
                        stub_reason(new_lines[line - 1], &planned.stem, stub_line)
                    })
                })
                .collect();
            let (kind, note) = if !reasons.is_empty() && reasons.iter().all(Option::is_some) {
                (ForeignEditKind::Stub, reasons[0].clone().unwrap_or_default())
            } else {
                (
                    ForeignEditKind::Fix,
                    touches.get(owner_stem).cloned().unwrap_or_default(),
                )
            };
            // @lfy def/generation/main.lfy:accept#accept:accept:673c8ca4213750333bbe2a3efcbf3c8c4f0effe89de65f4fdb92b2bb2ca0add3
            edits.push(ForeignEdit {
                by: planned.stem.clone(),
                owner: owner_stem.clone(),
                path: path.to_string(),
                kind,
                region: old_marker.clone(),
                before: lines_of(&old_lines, old_marker),
                after: lines_of(&new_lines, new_marker),
                note,
            });
        }
    }

    // The fix cap, and the note a fix needs; a stub needs neither.
    // @lfy def/generation/main.lfy:accept#accept:accept:4638ec2f3c3da87201f9123091fb0e8e54d466a9b5173afcc85f622fc1b79852
    let reach = &request.reach;
    let mut owners: Vec<&str> = Vec::new();
    for edit in edits.iter().filter(|edit| edit.kind == ForeignEditKind::Fix) {
        let changed = edit.changed_lines();
        // @lfy def/generation/main.lfy:accept#accept:accept:74b5b83a0e812b11c206618586026619959622840bbeca19fa01551901d0dbee
        if changed > reach.fix_lines {
            problems.push(format!(
                "{}: the fix in the region {} changes {changed} lines, more than the cap of {} \
                 lines a fix may change in one region; a bigger edit is a stub",
                edit.path,
                region_of(&edit.region),
                reach.fix_lines
            ));
        }
        if !owners.contains(&edit.owner.as_str()) {
            owners.push(&edit.owner);
        }
    }
    for owner in owners {
        // @lfy def/generation/main.lfy:accept#accept:accept:eac5ff490e6821fa4ae5d4cc225256fe2a980cc0b29cbd5b7b14a9e254b5f749
        let changed: usize = edits
            .iter()
            .filter(|edit| edit.kind == ForeignEditKind::Fix && edit.owner == owner)
            .map(ForeignEdit::changed_lines)
            .sum();
        if changed > reach.fix_unit_lines {
            problems.push(format!(
                "the fixes in the code of {owner} change {changed} lines, more than the cap of {} \
                 lines across the regions of one unit; a bigger edit is a stub",
                reach.fix_unit_lines
            ));
        }
        // @lfy def/generation/main.lfy:accept#accept:accept:41e970a839c8e929087ba7d9bce5cfeef8a74241a4acf8c4553e13080b8b99a2
        if touches.get(owner).is_none_or(|note| note.trim().is_empty()) {
            problems.push(format!(
                "the run fixed the code of {owner} and gave no note for it: a fix needs a note \
                 given through elfie_touch"
            ));
        }
    }
    edits
}

/// The markers of a text that name a file of the program, each with the region it begins.
// @lfy def/generation/main.lfy:accept#accept:accept:685361d7b868af843078c8f7efbabf1fcb62d73e6200f3553643c9b0133cb177
fn program_regions(workspace: &Workspace, text: &str) -> Vec<Marker> {
    parse_markers(text)
        .into_iter()
        .filter(|marker| workspace.file(&marker.file).is_some())
        .collect()
}

/// The region holding a line, counted from 1, with its order among the regions whose markers
/// name the same; `None` when no region does.
// @lfy def/generation/main.lfy:accept#accept:accept:685361d7b868af843078c8f7efbabf1fcb62d73e6200f3553643c9b0133cb177
fn region_holding(regions: &[Marker], line: usize) -> Option<(usize, &Marker)> {
    let marker = regions.iter().find(|marker| marker.covers(line))?;
    Some((ordinal_of(regions, marker), marker))
}

/// The order of a region among the regions whose markers name the same.
// @lfy def/generation/main.lfy:accept
fn ordinal_of(regions: &[Marker], marker: &Marker) -> usize {
    regions
        .iter()
        .filter(|other| other.output_line < marker.output_line && same_region(other, marker))
        .count()
}

/// The region of the given order among those whose markers name the same as `marker`.
// @lfy def/generation/main.lfy:accept
fn nth_like(regions: &[Marker], marker: &Marker, ordinal: usize) -> Option<Marker> {
    regions
        .iter()
        .filter(|other| same_region(other, marker))
        .nth(ordinal)
        .cloned()
}

/// The unit of the target whose region a marker begins: a unit whose own marker it is, and,
/// of two, the one an entity it names moved into rather than the one whose file declares it.
// @lfy def/generation/main.lfy:accept#accept:accept:68cbc9770e3159775eb89fd93f184936bf7ac960196d0cf2094957f125ab4ecc
fn owner_of(workspace: &Workspace, plan: &Plan, target: usize, marker: &Marker) -> Option<usize> {
    let owners: Vec<usize> = (0..plan.units.len())
        .filter(|&unit| {
            plan.units[unit].target == target && is_own_marker(workspace, plan, unit, marker)
        })
        .collect();
    owners
        .iter()
        .copied()
        .find(|&unit| workspace.files[plan.units[unit].file].path != marker.file)
        .or(owners.first().copied())
}

/// The lines a line diff removes from `old` and adds in `new`, each counted from 1 in its
/// own text, the diff keeping a longest common subsequence of the two.
// @lfy def/generation/main.lfy:accept#accept:accept:0a1470345711d81632a94e78c934b73d81c67b8564e43fcffb6a91c40b535f73
pub(super) fn line_diff(old: &[&str], new: &[&str]) -> (Vec<usize>, Vec<usize>) {
    // What the two share at either end is never changed, and leaves a small table between.
    let head = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_middle = &old[head..old.len() - tail];
    let new_middle = &new[head..new.len() - tail];
    let (rows, columns) = (old_middle.len(), new_middle.len());
    let mut table = vec![0u32; (rows + 1) * (columns + 1)];
    let at = |row: usize, column: usize| row * (columns + 1) + column;
    for row in (0..rows).rev() {
        for column in (0..columns).rev() {
            table[at(row, column)] = if old_middle[row] == new_middle[column] {
                table[at(row + 1, column + 1)] + 1
            } else {
                table[at(row + 1, column)].max(table[at(row, column + 1)])
            };
        }
    }
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    let (mut row, mut column) = (0, 0);
    while row < rows || column < columns {
        if row < rows && column < columns && old_middle[row] == new_middle[column] {
            row += 1;
            column += 1;
        } else if column < columns
            && (row == rows || table[at(row, column + 1)] >= table[at(row + 1, column)])
        {
            added.push(head + column + 1);
            column += 1;
        } else {
            removed.push(head + row + 1);
            row += 1;
        }
    }
    (removed, added)
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
