// @lfy def/generation/main.lfy:review
//! Everything a verifier is handed, and what it found.

use super::*;

/// Everything a verifier is handed to check the outputs of one batch against every
/// criterion and test whose verdict is not carried.
///
/// The instructions name the batch and the target, then, unit by unit, every entity with a
/// criterion or test under review: its name, its definition, where each of its regions is,
/// and each of those criteria and tests on one line with its id and its place — the file of
/// its origin relative to the workspace root, a colon, and the line its origin begins on —
/// then, for that unit, the criteria and tests of its own file, and last the verifier
/// protocol. No line of any output is quoted: a region is named, and the verifier pulls its
/// code through the agent server. Only local criteria and tests are reviewed here; a global
/// one is reviewed once, by [`global_review`].
///
/// `records` is what the batch's units were last reviewed as, as [`reviews_of`] gives them.
/// A criterion or test is listed when nothing says its verdict still holds: it has no
/// record, its last verdict was not satisfied, or the regions of its entity hash to
/// something other than what it was judged against. One whose entity generated no region,
/// and one of a unit's own file, is always listed and never carried, since there is no code
/// of its own whose sameness could stand for the verdict. With nothing left to list, the
/// instructions say so and hold no protocol.
///
/// `directory` is the directory the batch's outputs were written in, which the regions a
/// hash is taken over are read from, and `None` means the workspace root.
// Decision: the verifier is told where the regions are and pulls their code through the
// agent server, so a request stays small however large the batch's outputs are, and its
// protocol allows no edits: a review says where and why, never how to fix.
// Decision: a batch compiled in a copy of the root is reviewed in that copy, so the regions
// a verifier is pointed at, and the hashes its verdicts are recorded with, are those of the
// outputs it is judging and never the root's older ones.
// Decision: a criterion or test is reviewed again only when nothing says its verdict still
// holds; a change to code outside the entity's regions, such as a helper no marker names,
// changes no hash and so carries every verdict of that entity.
// Decision: a marker comment is no part of a region's hash, while the code on its line is,
// so moving a reworded criterion's markers to its new id, or adding a marker, does not send
// the other criteria of the entity to review again. A reworded criterion is reviewed all the
// same: its new text may ask for something else, so no verdict on the id it replaces stands
// for it.
// Decision: the definition passes the plan and the batch; a plan here does not own its
// workspace or the program lowered from it, so the program is an extra first parameter.
// @lfy def/generation/main.lfy:review#review:review:b999cb65ba87abe04347ef342c28070a2ffdbbee95641f03c9de5cea314a33ea
// @lfy def/generation/main.lfy:review#review:review:67735b4dd73460a1ce9714a4d61445247a01cff8daca1989651bea577a7f50e0
pub fn review(
    program: &Program,
    plan: &Plan,
    batch: &Batch,
    source_maps: &[SourceMap],
    directory: Option<&Path>,
    records: &[ReviewRecord],
) -> ReviewRequest {
    let workspace = &program.workspace;
    let model = &workspace.model;
    // @lfy def/generation/main.lfy:review#review:review:1c072387bd071f939c216da6d4ce6239cccd17ffa2df2f585223778cc3680fca
    let directory = directory.unwrap_or(workspace.root.as_path());
    let of_target = batch
        .units
        .first()
        .map(|&index| &workspace.targets[plan.units[index].target]);
    let target = of_target
        .map(|target| target.identifier.as_str())
        .unwrap_or_default();
    // How a line comment of the target's language begins, which is where a marker comment
    // begins and so where the part of a line no hash is taken over begins.
    let marker_comment = of_target
        .map(|target| target.marker_comment.as_str())
        .unwrap_or_default();

    // Unit by unit, entity by entity: the regions of the entity, the hash they come to, and
    // which of its criteria and tests that hash leaves to review.
    let mut asked: Vec<String> = Vec::new();
    let mut hashes: BTreeMap<String, String> = BTreeMap::new();
    let mut carried: Vec<ReviewRecord> = Vec::new();
    let mut sections = String::new();
    for &index in &batch.units {
        let unit = &plan.units[index];
        let mut body = String::new();
        for &entity in &unit.entities {
            let regions = entity_regions(workspace, source_maps, entity, target);
            // @lfy def/generation/main.lfy:review#review:review:e0bf60aeb1708754dc00bf7d49700c16b8706fbcf483a13b9f830882be426100
            let hash = source_hash(&regions_text(directory, marker_comment, &regions));
            let requirements = reviewed_requirements(program, unit.lowered, entity);
            let listed = listed_requirements(
                &requirements,
                &hash,
                records,
                !regions.is_empty(),
                &mut hashes,
                &mut carried,
            );
            // @lfy def/generation/main.lfy:review#review:review:bb873e216e23bb2a7659d3b1888588387a1f7980df4fac536938ec32d6d42b8d
            if listed.is_empty() {
                continue;
            }
            let (criteria, tests) = split_requirements(&listed);
            write_review_entity(&mut body, model, entity, &regions, &criteria, &tests);
            asked.extend(ids_of(&criteria));
            asked.extend(ids_of(&tests));
        }
        // A criterion or test of the unit's own file answers for no entity's code, so it has
        // no region of its own, hashes to the empty text, and is always listed.
        // @lfy def/generation/main.lfy:review#review:review:efbaab9e13ed7569408b035ce9b34392a9e0b4fa0a274b747ea27c0ef681cd65
        // @lfy def/generation/main.lfy:review#review:review:944917284f9b6da4a52ad0fe6ba57ee0440f72ee59ecdfdc8b90585e6e1e7417
        let file = &program.files[unit.lowered];
        let own: Vec<Requirement> = file
            .criteria
            .iter()
            .cloned()
            .map(Requirement::Criterion)
            .chain(file.tests.iter().cloned().map(Requirement::Test))
            .collect();
        let listed = listed_requirements(
            &own,
            &source_hash(""),
            records,
            false,
            &mut hashes,
            &mut carried,
        );
        if !listed.is_empty() {
            let (criteria, tests) = split_requirements(&listed);
            let _ = writeln!(body, "#### `{}`\n", workspace.files[unit.file].path);
            write_reviewed(&mut body, model, "Criteria", &criteria);
            write_reviewed(&mut body, model, "Tests", &tests);
            asked.extend(ids_of(&criteria));
            asked.extend(ids_of(&tests));
        }
        if !body.is_empty() {
            let _ = writeln!(
                sections,
                "### `{}` (stem `{}`)\n",
                workspace.files[unit.file].path, unit.stem
            );
            sections.push_str(&body);
        }
    }

    // A heading naming the batch and the target, then what is under review, then the
    // protocol; with nothing under review, the heading and one line saying so.
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Reviewing the batch `{}` for the target `{}`\n",
        batch.identifier, target
    );
    if asked.is_empty() {
        // @lfy def/generation/main.lfy:review#review:review:5133eddf9b981ba9bac41113779c58b491cedfbc67e2e3742ac236b381a254dc
        out.push_str(
            "Nothing in the batch needs review: every criterion and test of its units was \
             found satisfied against the code it still stands on.\n",
        );
    } else {
        // @lfy def/generation/main.lfy:review#review:review:f3ebae56b7c2144a9edb3e3b69ac07f174e3974f258284e909f2e10068ec9831
        out.push_str(
            "Every criterion and test below is local to one entity of this batch. A global \
             one is reviewed once, on its own, across every output of the program.\n\n",
        );
        out.push_str("## What was asked\n\n");
        out.push_str(&sections);
        // The verifier protocol.
        out.push_str(PROTOCOL);
    }

    // @lfy def/generation/main.lfy:review#review:review:197b2d0e6f7458d538f3cc8d8568be98a60636ca07026cd2eeb1d5933910f156
    ReviewRequest {
        batch: Some(batch.clone()),
        instructions: out,
        asked,
        hashes,
        carried,
    }
}

/// Every region one entity of a unit generated: each marker [`regions_of`] finds for its
/// name among the source maps of the batch's target whose file is the file that declares the
/// entity, in output order and then in output line order.
///
/// Only what the file declaring the entity generated, so an entity of another file with the
/// same name brings none of its code, while an entity moved into the unit brings its own,
/// which its declaring file marks.
// @lfy def/generation/main.lfy:review#review:review:0f43e9772c4aad39520e42801704e6fe6e9115829e267701e251b74908849820
// @lfy def/generation/main.lfy:review#review:review:e67f26bdccf8eaa77ebbfe85b9da0b83bf73bff7b2cad901af69d8b4797ad76e
fn entity_regions<'m>(
    workspace: &Workspace,
    source_maps: &'m [SourceMap],
    entity: EntityId,
    target: &str,
) -> Vec<(&'m SourceMap, &'m Marker)> {
    let model = &workspace.model;
    let declaring = declaring_path(model, entity);
    let name = qualified_name(model, entity);
    let mut out: Vec<(&SourceMap, &Marker)> =
        matching_regions(workspace, source_maps, &name, Some(target))
            .into_iter()
            .filter(|(_, marker)| declaring == Some(marker.file.as_str()))
            .collect();
    out.sort_by(|(left_map, left), (right_map, right)| {
        left_map
            .output
            .cmp(&right_map.output)
            .then(left.output_line.cmp(&right.output_line))
    });
    out
}

/// The text the hash of one entity's criteria and tests is taken over: the lines of each
/// region in its own output, one region after another, each line followed by a line break,
/// each with the marker comment it carries and the white space before it taken off and every
/// line that is then empty left out. A region whose output cannot be read adds nothing.
///
/// A marker comment says where code came from and which criterion it answers for, never what
/// the code does, so moving a reworded criterion's markers to its new id, or writing a
/// marker that was missing, leaves every hash of the entity as it was and sends no other
/// criterion of it back to review. The code on the line a marker trails is kept, since that
/// is code.
///
/// `directory` is the directory the outputs were written in, which each region is read from,
/// and `marker_comment` is how a line comment of the target's language begins.
// @lfy def/generation/main.lfy:review#review:review:e0bf60aeb1708754dc00bf7d49700c16b8706fbcf483a13b9f830882be426100
// @lfy def/generation/main.lfy:review#review:review:b20a4c21286c959c6a77873bc8d178ac43d720cb03877030c893be7f015f5903
fn regions_text(
    directory: &Path,
    marker_comment: &str,
    regions: &[(&SourceMap, &Marker)],
) -> String {
    let mut out = String::new();
    for (map, marker) in regions {
        let Ok(text) = std::fs::read_to_string(directory.join(&map.output)) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        push_region(&lines, marker, marker_comment, &mut out);
    }
    out
}

/// Which of one entity's criteria and tests a review lists, with every one of them recorded
/// in `hashes` under the hash of that entity's regions and every one it leaves out recorded
/// in `carried`.
///
/// `has_region` is whether the entity generated any region at all: with none, there is no
/// code whose sameness could stand for a verdict, so nothing is carried.
///
/// A record is matched by its id alone, so a criterion that rewords another is never carried
/// on the verdict the id it replaces earned, however little the code of its entity moved:
/// the new text may ask for something the code does not do.
// @lfy def/generation/main.lfy:review#review:review:5ff1db3eab7473b565a623216b6a86f264541a52a33fc4c939e828c7f537c783
// @lfy def/generation/main.lfy:review#review:review:32e025cd22c3a749bbc3afba52f86cb9b26ac41d0f4a822a6e5e00639e557312
// @lfy def/generation/main.lfy:review#review:review:944917284f9b6da4a52ad0fe6ba57ee0440f72ee59ecdfdc8b90585e6e1e7417
// @lfy def/generation/main.lfy:review#review:review:cfa867e320d611cec123830eb03c4368327c6de40443d7d3c904f20f1fb214d2
fn listed_requirements<'r>(
    requirements: &'r [Requirement],
    hash: &str,
    records: &[ReviewRecord],
    has_region: bool,
    hashes: &mut BTreeMap<String, String>,
    carried: &mut Vec<ReviewRecord>,
) -> Vec<&'r Requirement> {
    let mut out: Vec<&Requirement> = Vec::new();
    for requirement in requirements {
        hashes.insert(requirement.id().to_string(), hash.to_string());
        let held = records
            .iter()
            .find(|record| record.id == requirement.id())
            .filter(|record| {
                has_region && record.status == ReviewStatus::Satisfied && record.hash == hash
            });
        match held {
            Some(record) => carried.push(record.clone()),
            None => out.push(requirement),
        }
    }
    out
}

/// Criteria and tests apart, each in the order they were given.
// @lfy def/generation/main.lfy:review
fn split_requirements<'r>(
    requirements: &[&'r Requirement],
) -> (Vec<&'r Requirement>, Vec<&'r Requirement>) {
    requirements
        .iter()
        .copied()
        .partition(|requirement| matches!(requirement, Requirement::Criterion(_)))
}

/// The ids of criteria or tests, in the order they were given.
// @lfy def/generation/main.lfy:review
fn ids_of(requirements: &[&Requirement]) -> Vec<String> {
    requirements
        .iter()
        .map(|requirement| requirement.id().to_string())
        .collect()
}

/// Every local criterion and test the declaration of one entity of a unit holds: its own and
/// those of its members, criteria before tests at each level, taken from the unit's own
/// lowered file or, for an entity moved into the unit, from the file it came from.
// Decision: a member's criteria are reviewed under the declaration that holds it, as
// `regionsOf` gives a member's regions to its owner, so every local criterion and test of a
// unit is listed under one of its entities or under its own file and none goes unasked.
// @lfy def/generation/main.lfy:review#review:review:f3ebae56b7c2144a9edb3e3b69ac07f174e3974f258284e909f2e10068ec9831
fn reviewed_requirements(program: &Program, lowered: usize, entity: EntityId) -> Vec<Requirement> {
    let node = lowered_node_of(&program.files[lowered], entity)
        .or_else(|| lowered_declaration(program, entity));
    let mut out: Vec<Requirement> = Vec::new();
    if let Some(node) = node {
        requirements_within(node, &mut out);
    }
    out
}

/// The criteria and tests of one declaration, appended: those of the node, criteria before
/// tests, then those of the nodes within it, which are its members.
// @lfy def/generation/main.lfy:review
fn requirements_within(node: &LoweredNode, out: &mut Vec<Requirement>) {
    out.extend(node.criteria.iter().cloned().map(Requirement::Criterion));
    out.extend(node.tests.iter().cloned().map(Requirement::Test));
    for child in node.nodes() {
        requirements_within(child, out);
    }
}

/// One entity of a unit for a verifier: its name, its definition, where each of its regions
/// is, and then each criterion and each test under review on one line prefixed by its id and
/// its place.
// @lfy def/generation/main.lfy:review#review:review:b999cb65ba87abe04347ef342c28070a2ffdbbee95641f03c9de5cea314a33ea
fn write_review_entity(
    out: &mut String,
    model: &Model,
    entity: EntityId,
    regions: &[(&SourceMap, &Marker)],
    criteria: &[&Requirement],
    tests: &[&Requirement],
) {
    let record = &model.entities[entity];
    let _ = writeln!(
        out,
        "#### `{}` ({})\n",
        qualified_name(model, entity),
        kind_text(record)
    );
    if let Some(definition) = &record.definition {
        let _ = writeln!(out, "Definition: {definition}\n");
    }
    write_joined_regions(out, regions);
    write_reviewed(out, model, "Criteria", criteria);
    write_reviewed(out, model, "Tests", tests);
}

/// Each criterion or test under review on one line: its id, its place, an em dash, and its
/// text.
// @lfy def/generation/main.lfy:review#review:review:b999cb65ba87abe04347ef342c28070a2ffdbbee95641f03c9de5cea314a33ea
fn write_reviewed(out: &mut String, model: &Model, heading: &str, requirements: &[&Requirement]) {
    let _ = writeln!(out, "{heading}:");
    if requirements.is_empty() {
        out.push_str("(none)\n\n");
        return;
    }
    for requirement in requirements {
        let (id, origin, text) = match requirement {
            Requirement::Criterion(criterion) => {
                (&criterion.id, criterion.origin, criterion.text())
            }
            Requirement::Test(test) => (&test.id, test.origin, test_text(test)),
        };
        let _ = writeln!(out, "`{id}` {} — {text}", origin_place(model, origin));
    }
    out.push('\n');
}

/// Where every region given is: the output path, a colon, the first output line, a dash, and
/// the last, one region per line. None of their code is written, so the instructions hold no
/// line of any output; the verifier reads a region through the agent server.
// @lfy def/generation/main.lfy:review#review:review:67735b4dd73460a1ce9714a4d61445247a01cff8daca1989651bea577a7f50e0
// @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:67735b4dd73460a1ce9714a4d61445247a01cff8daca1989651bea577a7f50e0
fn write_regions(out: &mut String, regions: &[(&SourceMap, &Marker)]) {
    if regions.is_empty() {
        out.push_str("No region of any output was generated for it.\n\n");
        return;
    }
    out.push_str("Regions:\n");
    for (map, marker) in regions {
        let _ = writeln!(out, "`{}:{}-{}`", map.output, marker.output_line, marker.end);
    }
    out.push('\n');
}

/// The same, with a region that begins on the line after the one before it in the same
/// output joined to it as one range: a verifier reading one entity's code sees the runs of
/// lines it actually holds, rather than one range per marker where two markers happen to be
/// adjacent.
///
/// The regions come in output order and then in output line order, so a region adjacent to
/// the one before it is always the one it can be joined to.
// @lfy def/generation/main.lfy:review#review:review:b999cb65ba87abe04347ef342c28070a2ffdbbee95641f03c9de5cea314a33ea
fn write_joined_regions(out: &mut String, regions: &[(&SourceMap, &Marker)]) {
    if regions.is_empty() {
        out.push_str("No region of any output was generated for it.\n\n");
        return;
    }
    let mut ranges: Vec<(&str, usize, usize)> = Vec::new();
    for (map, marker) in regions {
        match ranges.last_mut() {
            Some((output, _, end))
                if *output == map.output.as_str() && marker.output_line == *end + 1 =>
            {
                *end = marker.end.max(*end);
            }
            _ => ranges.push((map.output.as_str(), marker.output_line, marker.end)),
        }
    }
    out.push_str("Regions:\n");
    for (output, first, last) in ranges {
        let _ = writeln!(out, "`{output}:{first}-{last}`");
    }
    out.push('\n');
}

// ---------------------------------------------------------------------------------------
// globalReview
// ---------------------------------------------------------------------------------------

/// Everything a verifier is handed to check every global criterion and test once, across
/// every output of the program.
///
/// The instructions name the global review and the targets, then give each global criterion
/// and then each global test with its id and its place, its text, and where every region
/// whose marker answers for its id is; one that no marker of any output names is listed with
/// a line saying no unit answered for it, so the verifier finds it unverifiable, never
/// violated. No line of any output is quoted, and nothing is carried: a global criterion is
/// asked about every time the review runs. The caller runs it once after every batch of a
/// compile, and whenever the program's global criteria or tests changed; a review it finds
/// violated gives the stems of the units whose markers answered for it, for the `violated`
/// argument of [`plan`].
// Decision: the definition passes the plan; a plan here does not own the program lowered
// from its workspace, and nothing else of the plan is read, so the program stands in for it.
// @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e0b38a1741c79bbd7d207fe9104c4f01ee115f0ccc6e3c9d2c738d8a30bac858
pub fn global_review(program: &Program, source_maps: &[SourceMap]) -> ReviewRequest {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let mut out = String::new();

    // A heading naming the global review and the targets.
    // @lfy def/generation/main.lfy:globalReview
    let targets: Vec<String> = workspace
        .targets
        .iter()
        .map(|target| format!("`{}`", target.identifier))
        .collect();
    let _ = writeln!(
        out,
        "# The global review of every criterion and test of the whole program\n"
    );
    let _ = writeln!(
        out,
        "The targets are {}.\n",
        if targets.is_empty() {
            "(none)".to_string()
        } else {
            targets.join(", ")
        }
    );

    let globals: Vec<Requirement> = program
        .criteria
        .iter()
        .cloned()
        .map(Requirement::Criterion)
        .chain(program.tests.iter().cloned().map(Requirement::Test))
        .collect();
    if globals.is_empty() {
        out.push_str("## What was asked\n\nThe program has no global criterion and no global test.\n\n");
    }
    // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:3163f899ae0e06b99cc3cfbdf7857a8f62a41a8e5685323600713e1e6347797b
    let mut asked: Vec<String> = Vec::new();
    for requirement in &globals {
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e0b38a1741c79bbd7d207fe9104c4f01ee115f0ccc6e3c9d2c738d8a30bac858
        let (id, origin, text) = match requirement {
            Requirement::Criterion(criterion) => {
                (&criterion.id, criterion.origin, criterion.text())
            }
            Requirement::Test(test) => (&test.id, test.origin, test_text(test)),
        };
        asked.push(id.clone());
        let _ = writeln!(out, "## `{id}` {}\n", origin_place(model, origin));
        let _ = writeln!(out, "{text}\n");
        // Every region whose marker answers for this id, whatever target it was built for.
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e0b38a1741c79bbd7d207fe9104c4f01ee115f0ccc6e3c9d2c738d8a30bac858
        let regions: Vec<(&SourceMap, &Marker)> = source_maps
            .iter()
            .flat_map(|map| {
                map.markers
                    .iter()
                    .filter(|marker| marker.requirement.as_deref() == Some(id.as_str()))
                    .map(move |marker| (map, marker))
            })
            .collect();
        if regions.is_empty() {
            // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:602d5d9913771ebb9a5ab23bc1b03ed23f9e22888e45cf65e826b7cfdbcc02e2
            out.push_str("No unit answered for it, so it cannot be checked against any region.\n\n");
            continue;
        }
        write_regions(&mut out, &regions);
    }

    // The verifier protocol.
    // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e0b38a1741c79bbd7d207fe9104c4f01ee115f0ccc6e3c9d2c738d8a30bac858
    out.push_str(PROTOCOL);

    ReviewRequest {
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:de84b01e368319ebde8eb693c9242574f7b87c6cdedc5229f53fbdf2a3cdd11d
        batch: None,
        instructions: out,
        asked,
        // A global criterion stands on the whole program rather than on the regions of one
        // entity, so there is no hash of its own code to carry a verdict on.
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e9cc8a4c5b45b3440c827bd67a1fd514c12870041202d0cd54fad21179548843
        hashes: BTreeMap::new(),
        carried: Vec::new(),
    }
}

/// What a verifier is told about its own work: that it examines and never edits, how it
/// reads the code of a region, what it submits for each criterion and each test, and how its
/// final message ends.
// Decision: the protocol asks for the place as well as the id, though [`read_review`]
// derives the place from the id and never from what was submitted; a verifier that copies
// the place it was given names its criterion in a way a person can read, and one that
// garbles the id is still placed by the file and line it copied.
// @lfy def/generation/main.lfy:review
// @lfy def/generation/main.lfy:globalReview
// @lfy def/generation/data.lfy:ReviewRequest#ReviewRequest:ReviewRequest:5e59c550936dcf7bf2e5d061524a9f594d0fad8ec3bcde714954a0e266066d26
const PROTOCOL: &str = "\
## How to review

You examine and never edit: you change no file, and you propose no code.

The code of a region is read through `elfie_output`: by the id of a criterion or test, by \
the name of an entity, or by reading the lines of the output file the region names.

Submit your verdicts as you judge them, many to a turn: one call of `elfie_reportMany` with \
one JSON line per verdict, or several calls of `elfie_report` in the same turn. Never one \
verdict per turn. Each verdict holds:

- `id` is copied from the id the criterion or test is given above, or left out when you \
  give `file` and `line` instead: the two halves of the place given with it above, the path \
  before the colon and the number after it.
- `status` is `satisfied` when the code does what the criterion says, `violated` when it \
  does something else, or `unverifiable` when no region can be checked against it. A \
  criterion no region can be checked against is unverifiable, never violated.
- `evidence` is the output path and the line range you read, such as \
  `crates/elfie-core/src/x.rs:120-134`, and empty when you read none.
- `note` is one line saying why and, for a violated one, what the code does instead.

The tools of the agent server are called before files are searched or read by hand: \
`elfie_source` for where a line of generated code came from, `elfie_search` for the lines of \
the outputs that match a pattern, such as the body of a helper no marker names, \
`elfie_entity` for a definition with its criteria and tests, and `elfie_references` for \
where a name is used.

Where `elfie_report` is not offered, write each verdict in your final message instead, as \
one JSON object on one line with exactly the keys `id`, `file`, `line`, `status`, \
`evidence`, and `note`, holding what `elfie_report` would be given.

End your final message with exactly one line reading `ELFIE: REVIEWED`.
";

// ---------------------------------------------------------------------------------------
// reviewOf
// ---------------------------------------------------------------------------------------

/// What a verifier found, read mechanically from the verdicts it submitted and from its
/// report.
///
/// `recorded` is the reviews the verifier submitted through the agent server during the run,
/// in the order they were recorded, and empty when it submitted none. They come first, and a
/// line of the report naming a criterion one of them already names is dropped: it is neither
/// a review nor a problem.
///
/// The end line is the last line reading `ELFIE: REVIEWED`, with any whitespace around it;
/// lines after it are ignored. The lines read are those before it, or every line when there
/// is none, each with the whitespace around it removed. A line read beginning with `{` is a
/// review line, and every other line is ignored, whatever it says, so prose, headings, and
/// fences around the reviews are never problems. A review line that reads as an object
/// whose status names a member of [`ReviewStatus`], whatever its case, and that names a
/// criterion or test of the program is one review, in report order; every other review line
/// is a problem, as written. Of two that name the same criterion or test only the last is
/// kept, at its own place. With no end line the problems end with a line saying the report
/// did not end.
///
/// A review line names its criterion or test by its id, by an id a character short or long,
/// or — with no id, or with one that names nothing and is near no single one — by the file
/// and line where exactly one of them begins. Its file, line, and entity are derived from
/// that origin, as [`Review::at`] fills them.
// Decision: the report is read leniently and only what tries to be a review can be wrong,
// because a verifier that adds a sentence or a fence around its lines has still reviewed
// everything, and running a second verifier for that costs as much as the first.
// Decision: a verdict submitted through the agent server was checked when it arrived, so it
// wins over a line of the report for the same criterion; the report is read only for what
// was not submitted.
///
/// `asked` is the ids the review request gave the verifier, and empty when they are not
/// known; each of them no review names is missing.
// @lfy def/generation/main.lfy:reviewOf
pub fn review_of(
    report: &str,
    program: &Program,
    asked: &[String],
    recorded: &[Review],
) -> ReviewReport {
    /// What the problems end with when the report has no end line.
    const UNENDED: &str = "the report did not end";

    let placed = placed_requirements(program);
    let begun = requirements_by_place(&placed);
    let lines: Vec<&str> = report.lines().collect();
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:10e1a05e247051fa67e5af8f9351b3d6c08728d0642aa557109f7db27e01e1d6
    let end = lines.iter().rposition(|line| line.trim() == REVIEWED);
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:10e1a05e247051fa67e5af8f9351b3d6c08728d0642aa557109f7db27e01e1d6
    let body = match end {
        Some(index) => &lines[..index],
        None => &lines[..],
    };
    // What the verifier submitted during the run comes first, in the order it was recorded.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:9ebef2c6f21093486b00c8a0124de7e4c4262014632c2a66b0b47702c693b59d
    let mut reviews: Vec<Review> = Vec::new();
    for review in recorded {
        reviews.retain(|kept| kept.id != review.id);
        reviews.push(review.clone());
    }
    let submitted = reviews.len();
    let mut problems: Vec<String> = Vec::new();
    let mut unplaced: Vec<String> = Vec::new();
    // The reviews read from the report and the problems come in report order, as the body is
    // read line by line.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:fdcd21d1f06a140e2bfae307be2625b9c27d43f2b73e320c1f152a39fd9638bc
    for line in body {
        // Whitespace around a line is ignored wherever the line is compared or parsed.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:2bf986f1d26195d0e45f531c18f47a790716471b351423d3a52d37d04c898921
        let text = line.trim();
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:24c8a0d8aab48ace02aea35fdac897930db4e5ea8b8b4bd675b3bd00afefeac3
        if !text.starts_with('{') {
            continue;
        }
        match read_review(text, &placed, &begun) {
            // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:4bf722eab5ca52f5732c6c9f8f080d2efd0dbed5df5ea0319fa4c4a56cade02c
            // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:731b47ce4e9dbb551b71e00078efb0e67419160fb2a75436b70be87929a8b8a9
            // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ffcb26fa671fb0476f3effa3e247f546bf1144d511802cfdd3a726c5756fc018
            Some(review) => {
                // A criterion the verifier already submitted a verdict for was checked when
                // it arrived, so the line adds nothing and is no problem either.
                // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:42415f0b3dec0f93094d195e8abb7b6b4c6f2a363c4db117d8c0c9e8b3c5417e
                if reviews[..submitted]
                    .iter()
                    .any(|held| held.id == review.id)
                {
                    continue;
                }
                // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:19f540298a118815926304ba19a124afbedf6dedfe2303d1b51cb9b16e308600
                reviews.retain(|kept| kept.id != review.id);
                reviews.push(review);
            }
            // A line that reads as no object, that names no status of a review, or that
            // places itself nowhere is a problem as written.
            // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:cc8ee014686684d4965cbc76770ce45c98db561453c188050a7a3d3c4b345ad6
            // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:065297aa8507d1cce029b56f7ee5ede6af8c5913fb55092cd6433f748dcff490
            // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ebd07d63053ee05f2507003716efdd9c78f20b38aecdf60cca2c1874d85612ac
            None => {
                // A line that claims a violation and places it nowhere is kept apart as
                // well, so that no violation is lost to an id a verifier garbled.
                // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:d4b52386cd8dd2565497fdaaf59b6fcafe4852653e38d732b6f4a7f4cb2d42f2
                if claims_violation(text) {
                    unplaced.push(text.to_string());
                }
                problems.push(text.to_string());
            }
        }
    }
    if end.is_none() {
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:b6eda5717d7bcd79108423e0c0c000b75e1bd73f8bd2a124b3cefcb31a67bae1
        problems.push(UNENDED.to_string());
    }
    // Each id the review was asked about that no review names, in the order it was asked.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:a7bd850a8fc6e8bf19c7c580ea931ed26ea1ea26c2cad6c2ea48096aa93c9535
    let missing: Vec<String> = asked
        .iter()
        .filter(|id| !reviews.iter().any(|review| &&review.id == id))
        .cloned()
        .collect();
    ReviewReport {
        reviews,
        problems,
        unplaced,
        missing,
    }
}

/// Whether a review line that was read as no review still says its status is violated: its
/// text reads as an object whose status names [`ReviewStatus::Violated`], whatever its case.
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:d4b52386cd8dd2565497fdaaf59b6fcafe4852653e38d732b6f4a7f4cb2d42f2
fn claims_violation(line: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    let Some(status) = value.get("status").and_then(serde_json::Value::as_str) else {
        return false;
    };
    ReviewStatus::from_name(&status.trim().to_lowercase()) == Some(ReviewStatus::Violated)
}

/// One review from one review line: `None` unless the line reads as an object whose status
/// names a member of [`ReviewStatus`], whatever its case, and that names a criterion or
/// test of the program. Its evidence and note are taken as given, and empty when missing;
/// every other key is ignored.
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:03c66ae037b851c7cce98a476e8c2dc9fe0e1d51213bd0f884fca3d2bf00fdea
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:9edd00519d199e54062049e61315587483989f62a46fc5a31c9be591c90931ce
fn read_review(
    line: &str,
    placed: &BTreeMap<String, (String, usize, String)>,
    begun: &BTreeMap<(String, usize), String>,
) -> Option<Review> {
    let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let object = value.as_object()?;
    let spelled = object.get("status")?.as_str()?.to_ascii_lowercase();
    let status = ReviewStatus::from_name(&spelled)?;
    let id = named_requirement(object, placed, begun)?;
    let (file, at, entity) = placed.get(&id)?;
    let text = |key: &str| {
        object
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    Some(
        Review {
            id,
            file: String::new(),
            line: 0,
            entity: String::new(),
            status,
            evidence: text("evidence"),
            note: text("note"),
        }
        .at(file, *at, entity),
    )
}

/// Which criterion or test of the program a review line names: the one its id is; the one
/// exactly one id near it begins or is begun by, so an id copied a character short or long
/// still counts; or, failing both — with no id, or with one that is no id of the program and
/// near no single one — the one that begins alone where its file and line say.
// @lfy def/generation/main.lfy:reviewOf
fn named_requirement(
    object: &serde_json::Map<String, serde_json::Value>,
    placed: &BTreeMap<String, (String, usize, String)>,
    begun: &BTreeMap<(String, usize), String>,
) -> Option<String> {
    let given = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:4bf722eab5ca52f5732c6c9f8f080d2efd0dbed5df5ea0319fa4c4a56cade02c
    if placed.contains_key(given) {
        return Some(given.to_string());
    }
    // An id a character short or long names what it nearly is, as long as it nearly is one
    // thing only; an empty id is near every id, so it names nothing by this reading.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:731b47ce4e9dbb551b71e00078efb0e67419160fb2a75436b70be87929a8b8a9
    if !given.is_empty() {
        let mut near = placed.keys().filter(|id| is_near(id, given));
        if let Some(found) = near.next()
            && near.next().is_none()
        {
            return Some(found.clone());
        }
    }
    // With no id, or with one that names nothing and is near nothing or near more than one
    // thing, the place the line gives names it when exactly one begins there.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ffcb26fa671fb0476f3effa3e247f546bf1144d511802cfdd3a726c5756fc018
    let file = object.get("file")?.as_str()?.trim().to_string();
    let line = usize::try_from(object.get("line")?.as_u64()?).ok()?;
    begun.get(&(file, line)).cloned()
}

/// Whether two ids are the same one up to a character: one begins the other, and they share
/// their receiver, their contributor, and at least sixteen hex digits of their hash.
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:731b47ce4e9dbb551b71e00078efb0e67419160fb2a75436b70be87929a8b8a9
fn is_near(id: &str, given: &str) -> bool {
    /// How many hex digits of the hash two ids share to be the same one.
    const SHARED: usize = 16;

    if !id.starts_with(given) && !given.starts_with(id) {
        return false;
    }
    let Some((receiver, contributor, hash)) = id_parts(id) else {
        return false;
    };
    let Some((their_receiver, their_contributor, their_hash)) = id_parts(given) else {
        return false;
    };
    if receiver != their_receiver || contributor != their_contributor {
        return false;
    }
    hash.chars()
        .zip(their_hash.chars())
        .take_while(|(left, right)| left == right && left.is_ascii_hexdigit())
        .count()
        >= SHARED
}

/// The id of the criterion or test that begins at each place where exactly one of them
/// begins, as its file and its line; a place two begin at is left out, since a review line
/// with no id would name neither.
// @lfy def/generation/main.lfy:reviewOf
fn requirements_by_place(
    placed: &BTreeMap<String, (String, usize, String)>,
) -> BTreeMap<(String, usize), String> {
    let mut out: BTreeMap<(String, usize), Option<String>> = BTreeMap::new();
    for (id, (file, line, _)) in placed {
        out.entry((file.clone(), *line))
            .and_modify(|held| *held = None)
            .or_insert_with(|| Some(id.clone()));
    }
    out.into_iter()
        .filter_map(|(place, id)| id.map(|id| (place, id)))
        .collect()
}

/// Every criterion and test of a program by id, global and local, each with the file and
/// the line its origin gives and the name of the entity it belongs to.
// @lfy def/generation/main.lfy:reviewOf
fn placed_requirements(program: &Program) -> BTreeMap<String, (String, usize, String)> {
    let model = &program.workspace.model;
    let mut out: BTreeMap<String, (String, usize, String)> = BTreeMap::new();
    let mut place = |id: &str, origin: NodeRef, entity: Option<EntityId>| {
        let name = match entity {
            Some(entity) => qualified_name(model, entity),
            None => GLOBAL.to_string(),
        };
        out.insert(
            id.to_string(),
            (
                model.sources[origin.file].path.clone(),
                line_of(model, origin),
                name,
            ),
        );
    };
    for criterion in &program.criteria {
        place(&criterion.id, criterion.origin, criterion.entity);
    }
    for test in &program.tests {
        place(&test.id, test.origin, test.entity);
    }
    for file in &program.files {
        for requirement in local_requirements(file) {
            match requirement {
                Requirement::Criterion(criterion) => {
                    place(&criterion.id, criterion.origin, criterion.entity);
                }
                Requirement::Test(test) => place(&test.id, test.origin, test.entity),
            }
        }
    }
    out
}
