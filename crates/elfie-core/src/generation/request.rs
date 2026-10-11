// @lfy def/generation/main.lfy:request
//! Everything the compiler is handed to produce the outputs of one batch.

use super::*;

/// Everything the compiler is handed to produce the outputs of one batch.
///
/// The sources are the lowered text of each unit's file by path, and the requirements its
/// local criteria and tests, each with its id; the globals are every global criterion and
/// test of the program; `existing` is kept only where its path is among the outputs of a
/// unit of the batch; there is one interface per dependency of the batch that is not itself
/// in it; the guidance is every criterion of the target's declaration, of each of its
/// layers, and of every trait those layers extend, as [`guidance_for`] gathers them; the
/// native dependencies and the commands are the target's; and the knowledge is the
/// target's, then that of each entity of the batch's units, then that of each entity of an
/// interface, each item once. The instructions quote every criterion and test already
/// resolved, so a compiler with no access to the model can still work.
///
/// What was dropped and what was reworded are kept apart: a criterion that kept its place
/// and took a new id is a rewording, with the id and the text it replaces, while one the
/// unit no longer has at all is removed. A unit nothing else differs for is named in
/// `reworded_only`, so a caller may move its markers and ask no compiler at all.
///
/// `hosted` is whether the compiler reading the request runs in a compiler session of the
/// compile-hosted agent server, which offers `elfie_task`, `elfie_dependency`, `elfie_mark`,
/// and `elfie_submit`.
///
/// `hash_matched` holds the path of each unit whose entry in `previous` is its hash-matched
/// source rather than the source its outputs were accepted from. Such a text lowers to the
/// same code and says nothing about what the outputs answered for, so nothing of it is read
/// as a rewording, what was dropped is read off the recorded ids alone — with neither a place
/// nor a text, which that revision does not hold — and the unit is never one whose markers
/// are the whole of the work.
// Decision: the definition passes the plan and the batch; a plan here does not own its
// workspace or the program lowered from it, so the program is an extra first parameter.
// Decision: the caller says whether the request is for a hosted compiler session, because
// only the caller knows; a hosted compiler pulls an interface when it needs one, so its
// request names the interfaces instead of quoting them, while a request read anywhere else
// quotes them whole. A compiler command that never uses the session it is hosted in still
// gets the interfaces by name only, and reads each definition another way.
// @lfy def/generation/main.lfy:request
pub fn request(
    program: &Program,
    plan: &Plan,
    batch: &Batch,
    existing: &[Output],
    previous: &BTreeMap<String, String>,
    hosted: bool,
    hash_matched: &[String],
) -> Request {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let Some(&first) = batch.units.first() else {
        // Decision: an empty batch has no target, so there is nothing to say; the caller
        // never builds one, since `plan` only batches units it planned.
        return Request {
            batch: batch.clone(),
            instructions: String::new(),
            sources: BTreeMap::new(),
            requirements: BTreeMap::new(),
            globals: Vec::new(),
            previous: previous.clone(),
            answered: BTreeMap::new(),
            removed: BTreeMap::new(),
            reworded: BTreeMap::new(),
            reworded_only: Vec::new(),
            existing: BTreeMap::new(),
            interfaces: Vec::new(),
            guidance: Vec::new(),
            native_dependencies: Vec::new(),
            knowledge: Vec::new(),
            commands: Vec::new(),
        };
    };
    let target = &workspace.targets[plan.units[first].target];

    // @lfy def/generation/main.lfy:request
    let sources: BTreeMap<String, String> = batch
        .units
        .iter()
        .map(|&index| {
            let unit = &plan.units[index];
            (workspace.files[unit.file].path.clone(), unit.text.clone())
        })
        .collect();
    // @lfy def/generation/main.lfy:request
    let requirements: BTreeMap<String, Vec<Requirement>> = batch
        .units
        .iter()
        .map(|&index| {
            let unit = &plan.units[index];
            (
                workspace.files[unit.file].path.clone(),
                unit.requirements(program),
            )
        })
        .collect();
    // The file of each unit of the batch as it was when its outputs were accepted, lowered
    // the way its file is lowered now.
    // @lfy def/generation/main.lfy:request#request:request:e6a9a7f8f68b31456fa6f45f2971df1eddc6eed03b9bcb155872fff58c0ca957
    let was: BTreeMap<String, (String, Vec<RemovedRequirement>)> = batch
        .units
        .iter()
        .filter_map(|&index| {
            let path = &workspace.files[plan.units[index].file].path;
            let text = previous.get(path)?;
            let (lowered, requirements) = file_as_it_was(program, path, text)?;
            Some((path.clone(), (lowered, requirements)))
        })
        .collect();
    // The lowered text each unit's outputs were generated from, which the instructions read
    // against the lowered text of the unit now.
    // @lfy def/generation/main.lfy:request#request:request:6232735fed99654f7d25efa96687134e92c7667e0be166ba2471ca9d018c3103
    let previous_lowered: BTreeMap<String, String> = was
        .iter()
        .map(|(path, (lowered, _))| (path.clone(), lowered.clone()))
        .collect();
    // The ids the existing outputs were generated against: what the map file of the unit
    // recorded, or, where it recorded none, the ids of the file as it was.
    // @lfy def/generation/main.lfy:request#request:request:160056c645e3e69afd7ce0bb74137db9a5e5f286df0a983570f9e2569276b533
    // @lfy def/generation/main.lfy:request#request:request:17f8da3e2ff4913864923cd11e57fe5ff20121b6a364f38e388c25c9113dff54
    let answered: BTreeMap<String, Vec<String>> = batch
        .units
        .iter()
        .filter_map(|&index| {
            let path = &workspace.files[plan.units[index].file].path;
            let recorded = plan.units[index]
                .outputs
                .iter()
                .find(|map| !map.answered.is_empty())
                .map(|map| map.answered.clone());
            let ids = match recorded {
                Some(ids) => ids,
                None => was
                    .get(path)?
                    .1
                    .iter()
                    .map(|requirement| requirement.id.clone())
                    .collect(),
            };
            Some((path.clone(), ids))
        })
        .collect();
    // Each criterion and test of the unit that rewords one the outputs were generated
    // against: the same place among its entity's criteria or tests, another text, and so
    // another id. What was reworded is not gone, so it is kept apart from what is.
    // @lfy def/generation/data.lfy:Request.reworded
    // @lfy def/generation/main.lfy:request#request:request:de823e5d8fbff081ee17b88c1ae0477dff3832d78d636c05c9b9a8c90ae71da7
    let rewordings: BTreeMap<String, Vec<Rewording>> = batch
        .units
        .iter()
        .filter_map(|&index| {
            let unit = &plan.units[index];
            let path = &workspace.files[unit.file].path;
            let text = previous.get(path)?;
            // A hash-matched source is some other revision of the file that lowers to the
            // same code; where its criteria stood says nothing about what the outputs were
            // accepted against, so none of them is read as a rewording.
            // @lfy def/generation/main.lfy:request#request:request:68bd098b5b9b98c6a8f66b7bebfe21fd86dc2ead715c655d6f9f940a6408ff30
            if hash_matched.iter().any(|named| named == path) {
                return Some((path.clone(), Vec::new()));
            }
            Some((path.clone(), reworded(program, unit, text)))
        })
        .collect();
    // Each criterion and test of the file as it was that the unit no longer has and that
    // none of its own rewords, in the order the file as it was gives them: what a compiler
    // reading the existing code needs in order to tell code that answers for nothing any
    // more from code that still answers.
    // @lfy def/generation/data.lfy:Request.removed
    // @lfy def/generation/main.lfy:request#request:request:19eae5873e1ffc58511457bc5471084a486c137ebe615a8fdf95f3ac0fe43289
    let removed: BTreeMap<String, Vec<RemovedRequirement>> = batch
        .units
        .iter()
        .filter_map(|&index| {
            let path = &workspace.files[plan.units[index].file].path;
            if !previous.contains_key(path) {
                return None;
            }
            let held = requirements.get(path)?;
            // A hash-matched source holds another revision's criteria and tests, so what the
            // outputs answered for is only what their maps recorded: the ids alone, in id
            // order, with neither the place nor the text that revision does not hold.
            // @lfy def/generation/main.lfy:request#request:request:68bd098b5b9b98c6a8f66b7bebfe21fd86dc2ead715c655d6f9f940a6408ff30
            if hash_matched.iter().any(|named| named == path) {
                let mut gone: Vec<RemovedRequirement> = answered
                    .get(path)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .filter(|id| !held.iter().any(|other| other.id() == id.as_str()))
                    .map(|id| RemovedRequirement {
                        id: id.clone(),
                        place: String::new(),
                        text: String::new(),
                    })
                    .collect();
                gone.sort_by(|left, right| left.id.cmp(&right.id));
                return Some((path.clone(), gone));
            }
            let (_, before) = was.get(path)?;
            let rewords = rewordings.get(path).map(Vec::as_slice).unwrap_or_default();
            let gone: Vec<RemovedRequirement> = before
                .iter()
                .filter(|requirement| !held.iter().any(|other| other.id() == requirement.id))
                .filter(|requirement| {
                    !rewords
                        .iter()
                        .any(|rewording| rewording.replaces == requirement.id)
                })
                .cloned()
                .collect();
            Some((path.clone(), gone))
        })
        .collect();
    // A unit whose existing outputs differ from what it asks for now only in the ids of
    // reworded criteria and tests: its lowered code is unchanged, nothing was dropped and
    // nothing added, and every interface it was built against still hashes the same. Moving
    // its markers is then the whole of the work, which the caller may do without the
    // compiler.
    // @lfy def/generation/data.lfy:Request.rewordedOnly
    // @lfy def/generation/main.lfy:request#request:request:4a0dea1f763a8af062fa2366467ce5030e12d175192b8584660d6f3ace3eebc6
    // @lfy def/generation/main.lfy:request#request:request:d1ec465e091ae83573ab2ee5e7fe9ee86667ea44ffe505f8aefe88311e0071b7
    let reworded_only: Vec<String> = batch
        .units
        .iter()
        .filter_map(|&index| {
            let unit = &plan.units[index];
            let path = &workspace.files[unit.file].path;
            if unit.reason != Some(Reason::Requirements) {
                return None;
            }
            if !previous.contains_key(path) {
                return None;
            }
            // A hash-matched source is not the one the outputs were accepted from, so what
            // differs from what they were generated against is not known to be the ids alone.
            if hash_matched.iter().any(|named| named == path) {
                return None;
            }
            if !removed.get(path).is_some_and(Vec::is_empty) {
                return None;
            }
            let rewords = rewordings.get(path).filter(|rewords| !rewords.is_empty())?;
            let answered = answered.get(path).map(Vec::as_slice).unwrap_or_default();
            // Every criterion and test of the unit was either answered for under the id it
            // still has or rewords one that was.
            if !unit.requirements(program).iter().all(|requirement| {
                answered.iter().any(|id| id == requirement.id())
                    || rewords
                        .iter()
                        .any(|rewording| rewording.id == requirement.id())
            }) {
                return None;
            }
            // Every dependency's interface is the one the outputs were generated against,
            // so nothing of what the unit may rely on has moved either.
            if !unit.dependencies.iter().all(|&dependency| {
                let dependency = &plan.units[dependency];
                let signature = interface_signature(workspace, dependency);
                let of_path = &workspace.files[dependency.file].path;
                unit.outputs
                    .iter()
                    .all(|map| map.dependencies.get(of_path) == Some(&signature))
            }) {
                return None;
            }
            Some(path.clone())
        })
        .collect();
    // @lfy def/generation/main.lfy:request
    let globals: Vec<Requirement> = program
        .criteria
        .iter()
        .cloned()
        .map(Requirement::Criterion)
        .chain(program.tests.iter().cloned().map(Requirement::Test))
        .collect();
    // @lfy def/generation/main.lfy:request
    let existing: BTreeMap<String, String> = existing
        .iter()
        .filter(|output| {
            batch.units.iter().any(|&index| {
                plan.units[index]
                    .outputs
                    .iter()
                    .any(|map| map.output == output.path)
            })
        })
        .map(|output| (output.path.clone(), output.text.clone()))
        .collect();
    // One interface per dependency outside the batch, each holding every entity of that
    // dependency: what a hosted compiler is given by name is the same interface a request
    // read anywhere else quotes whole, so the request carries no less for being hosted.
    // @lfy def/generation/main.lfy:request
    // @lfy def/generation/main.lfy:request#request:request:5eb8b7bd8df8b28d5af1e781560a38611e0b664872e17a14d9c2e7352a7798cf
    let mut interfaces: Vec<Interface> = Vec::new();
    for &index in &batch.units {
        for &dependency in &plan.units[index].dependencies {
            if batch.units.contains(&dependency)
                || interfaces
                    .iter()
                    .any(|interface| interface.unit == dependency)
            {
                continue;
            }
            interfaces.push(Interface {
                unit: dependency,
                outputs: plan.units[dependency]
                    .outputs
                    .iter()
                    .map(|map| map.output.clone())
                    .collect(),
                entities: plan.units[dependency].entities.clone(),
            });
        }
    }
    let guidance = guidance_for(model, target);
    // @lfy def/generation/main.lfy:request
    let native_dependencies: Vec<NativeDependency> = target.native_dependencies.clone();
    let commands: Vec<Command> = target.commands.clone();
    // The target's knowledge, then that of each entity of the batch's units, then that of
    // each entity of an interface, each item once.
    // @lfy def/generation/main.lfy:request
    let mut knowledge: Vec<Knowledge> = Vec::new();
    let hold = |items: &[Knowledge], knowledge: &mut Vec<Knowledge>| {
        for item in items {
            if !knowledge.contains(item) {
                knowledge.push(item.clone());
            }
        }
    };
    hold(&target.knowledge, &mut knowledge);
    for &index in &batch.units {
        for &entity in &plan.units[index].entities {
            hold(&model.entities[entity].knowledge, &mut knowledge);
        }
    }
    for interface in &interfaces {
        for &entity in &interface.entities {
            hold(&model.entities[entity].knowledge, &mut knowledge);
        }
    }

    let instructions = instructions(
        program,
        plan,
        batch,
        target,
        hosted,
        &Quoted {
            sources: &sources,
            globals: &globals,
            previous: &previous_lowered,
            answered: &answered,
            removed: &removed,
            reworded: &rewordings,
            existing: &existing,
            interfaces: &interfaces,
            guidance: &guidance,
            native_dependencies: &native_dependencies,
            knowledge: &knowledge,
            commands: &commands,
        },
    );

    Request {
        batch: batch.clone(),
        instructions,
        sources,
        requirements,
        globals,
        previous: previous_lowered,
        answered,
        removed,
        reworded: rewordings,
        reworded_only,
        existing,
        interfaces,
        guidance,
        native_dependencies,
        knowledge,
        commands,
    }
}

/// What the instructions quote, gathered before they are written.
// Decision: the definition writes the instructions from the request it is building; the
// parts are grouped here rather than passed one by one.
// @lfy def/generation/main.lfy:request
struct Quoted<'r> {
    sources: &'r BTreeMap<String, String>,
    globals: &'r [Requirement],
    previous: &'r BTreeMap<String, String>,
    answered: &'r BTreeMap<String, Vec<String>>,
    removed: &'r BTreeMap<String, Vec<RemovedRequirement>>,
    reworded: &'r BTreeMap<String, Vec<Rewording>>,
    existing: &'r BTreeMap<String, String>,
    interfaces: &'r [Interface],
    guidance: &'r [Criterion],
    native_dependencies: &'r [NativeDependency],
    knowledge: &'r [Knowledge],
    commands: &'r [Command],
}

/// The guidance of a target: the criteria of its declaration that neither a layer nor a
/// trait a layer extends gave, in order; then those each of its layers contributed, layer by
/// layer in guidance order; then those of every trait the layers extend, transitively and
/// nearest first, each trait once.
///
/// Every template value is already rendered from the arguments the layer was applied with,
/// since applying a layer to the declaration resolved each of its criteria for that
/// application; a trait the layers extend whose criteria the declaration does not carry
/// still gives its own.
// Decision: the criteria are read off the declaration rather than off each trait, because a
// trait's own criteria keep a template value of its parameters as written — the parameters
// are unbound where the trait is declared — and the application that bound them is what the
// declaration holds.
// @lfy def/generation/main.lfy:request#request:request:3d9e3582187fd1f1532a46bf0f938c787311c81feb59f4a7d7d5b0da9a813a9a
pub fn guidance_for(model: &Model, target: &Target) -> Vec<Criterion> {
    let held = model::criteria_of(model, target.declaration);
    // A trait reached through two layers is inherited by the declaration once per path, so
    // what one contributor gave is taken once.
    // @lfy def/generation/main.lfy:request
    let once = |criteria: Vec<Criterion>| -> Vec<Criterion> {
        let mut out: Vec<Criterion> = Vec::new();
        for criterion in criteria {
            if !out.contains(&criterion) {
                out.push(criterion);
            }
        }
        out
    };
    // The layers, then every trait they extend, gathered before anything is taken, because
    // what comes first is what none of them gave. Nearest first: a trait is reached a whole
    // level after the trait that extends it, and a trait reached twice keeps its first
    // place.
    // @lfy def/generation/main.lfy:request
    let mut layered: Vec<EntityId> = Vec::new();
    for &layer in &target.layers {
        if layered.contains(&layer) {
            continue; // @lfy def/generation/main.lfy:request
        }
        layered.push(layer);
    }
    let mut reached = 0;
    while reached < layered.len() {
        let entity = layered[reached];
        reached += 1;
        for applied in &model.entities[entity].traits {
            if !matches!(applied.source, AppliedSource::Extends(_))
                || layered.contains(&applied.entity)
            {
                continue; // @lfy def/generation/main.lfy:request
            }
            layered.push(applied.entity);
        }
    }
    // Whoever else gave the declaration a criterion comes first: the declaration's own body,
    // or the file that adds one with an add call on the const's acceptanceCriteria.
    // @lfy def/generation/main.lfy:request
    let mut out: Vec<Criterion> = once(
        held.iter()
            .filter(|criterion| !layered.contains(&criterion.contributor))
            .cloned()
            .collect(),
    );
    let by = |contributor: EntityId| -> Vec<Criterion> {
        let mine = once(
            held.iter()
                .filter(|criterion| criterion.contributor == contributor)
                .cloned()
                .collect(),
        );
        if !mine.is_empty() {
            return mine;
        }
        once(
            model::criteria_of(model, contributor)
                .into_iter()
                .filter(|criterion| criterion.contributor == contributor)
                .collect(),
        )
    };
    // @lfy def/generation/main.lfy:request
    for &entity in &layered {
        out.extend(by(entity));
    }
    out
}

/// The prompt: what to produce, where, the rules for producing it, and how to report the
/// outcome, in the order the definition lists.
// @lfy def/generation/main.lfy:request
fn instructions(
    program: &Program,
    plan: &Plan,
    batch: &Batch,
    target: &Target,
    hosted: bool,
    quoted: &Quoted<'_>,
) -> String {
    let workspace = &program.workspace;
    let Quoted {
        sources,
        globals,
        previous: _,
        answered,
        removed,
        reworded,
        existing: _,
        interfaces,
        guidance,
        native_dependencies,
        knowledge,
        commands,
    } = *quoted;
    let model = &workspace.model;
    let mut out = String::new();

    // The reader and its job, and the units by stem.
    // @lfy def/generation/main.lfy:request#request:request:571bc891f6509a3d1997b846f685a316e34804763d8271373888f530426558b3
    // @lfy def/generation/main.lfy:request#request:request:d650f0548150afb04ee79524e308a0150e9cf8a3f4dcc9ffde74e3d90e7fcada
    let _ = writeln!(
        out,
        "# Compiling the batch `{}` for the target `{}`\n",
        batch.identifier, target.identifier
    );
    let _ = writeln!(
        out,
        "You are the compiler for the target `{}`. Your whole job is the outputs for the units of this \
         batch, and you write nothing outside them:\n",
        target.identifier
    );
    for &index in &batch.units {
        let unit = &plan.units[index];
        let _ = writeln!(
            out,
            "- `{}` (`{}`)",
            unit.stem, workspace.files[unit.file].path
        );
    }
    out.push('\n');

    // Where the outputs go.
    // @lfy def/generation/main.lfy:request#request:request:800766e448bfb8ed73777735dcb6d1a81a50d15783302ad27486b92c2d669f5e
    out.push_str("## Where the outputs go\n\n");
    let _ = writeln!(
        out,
        "The outputs go under `{}`. Each unit's file names are spelled from its stem as the guidance \
         says.\n",
        target.output_directory
    );
    for &index in &batch.units {
        let unit = &plan.units[index];
        if unit.outputs.is_empty() {
            let _ = writeln!(
                out,
                "- `{}`: nothing has been generated for it yet.",
                unit.stem
            );
        } else {
            // @lfy def/generation/main.lfy:request#request:request:08f6ffbf337200835b96675249633150fae7cb0d93486e08ccd3b5c9f63847c7
            let paths: Vec<String> = unit
                .outputs
                .iter()
                .map(|map| format!("`{}`", map.output))
                .collect();
            let _ = writeln!(
                out,
                "- `{}`: the last accepted generation produced {}; write to the same paths, except \
                 that a module the target's guidance says to split gains submodule files and has \
                 its code moved into them.",
                unit.stem,
                paths.join(", ")
            );
        }
    }
    out.push('\n');
    write_existing(&mut out, workspace, plan, batch, quoted);

    // The guidance.
    // @lfy def/generation/main.lfy:request#request:request:3d9e3582187fd1f1532a46bf0f938c787311c81feb59f4a7d7d5b0da9a813a9a
    out.push_str("## Guidance\n\n");
    let _ = writeln!(
        out,
        "The declaration of the target `{}`, each of its layers in guidance order, and every trait \
         those layers extend give this guidance for building against the target:\n",
        target.identifier
    );
    if guidance.is_empty() {
        out.push_str("(none)\n");
    }
    for criterion in guidance {
        write_criterion(&mut out, criterion);
    }
    out.push('\n');

    // The native dependencies. @lfy def/generation/main.lfy:request
    out.push_str("## Native dependencies\n\n");
    out.push_str("The generated code may require these from the target's ecosystem:\n");
    if native_dependencies.is_empty() {
        out.push_str("(none)\n");
    }
    for dependency in native_dependencies {
        match &dependency.version {
            Some(version) => {
                let _ = writeln!(
                    out,
                    "- `{}` ({}), version `{version}`",
                    dependency.identifier, dependency.ecosystem
                );
            }
            None => {
                let _ = writeln!(
                    out,
                    "- `{}` ({}), any version",
                    dependency.identifier, dependency.ecosystem
                );
            }
        }
    }
    out.push('\n');

    // The knowledge. @lfy def/generation/main.lfy:request
    write_knowledge(&mut out, workspace, knowledge);

    // Each interface: quoted whole, or named for a hosted compiler to pull.
    out.push_str("## Interfaces\n\n");
    if interfaces.is_empty() {
        out.push_str("The units of this batch depend on no unit outside it.\n\n");
    } else {
        // @lfy def/generation/main.lfy:request#request:request:7424b0fcb62916f23bfb6c1265f3a3f943fb1354558e0d4b0ea4722212bc0c7f
        out.push_str(
            "The units of this batch depend on the units below. Use their entities by the names their \
             outputs spell, and never edit their outputs.\n\n",
        );
        if hosted {
            // A hosted compiler pulls the interface of the one entity it needs, so the
            // request names the entities and spells none of them out.
            // @lfy def/generation/main.lfy:request#request:request:91c07c652cfc9edd8b82e5c1bebc5ef3998ae8e528cbd1623161418170399772
            out.push_str(
                "Read the interface of one of those entities — its kind, its definition, its type, its \
                 parameters, and its output — with `elfie_dependency`.\n\n",
            );
        }
    }
    for interface in interfaces {
        let dependency = &plan.units[interface.unit];
        if hosted {
            // @lfy def/generation/main.lfy:request#request:request:c5448d7c2bb4f497fbdd859f9d7405cd0bdf319c54990d943d33d646b6da87d4
            let paths: Vec<String> = interface
                .outputs
                .iter()
                .map(|path| format!("`{path}`"))
                .collect();
            let names: Vec<String> = interface
                .entities
                .iter()
                .map(|&entity| format!("`{}`", qualified_name(model, entity)))
                .collect();
            let _ = writeln!(
                out,
                "- `{}`: outputs {}; entities {}",
                workspace.files[dependency.file].path,
                if paths.is_empty() {
                    "(not generated yet)".to_string()
                } else {
                    paths.join(", ")
                },
                if names.is_empty() {
                    "(none)".to_string()
                } else {
                    names.join(", ")
                }
            );
            continue;
        }
        // @lfy def/generation/main.lfy:request#request:request:1530126fe56400a7920d4c5c53eb10a8785a3ed3921d00bc2814a0452615fd57
        let _ = writeln!(out, "### `{}`\n", workspace.files[dependency.file].path);
        out.push_str("Outputs:\n");
        if interface.outputs.is_empty() {
            out.push_str("- (not generated yet)\n");
        }
        for path in &interface.outputs {
            let _ = writeln!(out, "- `{path}`");
        }
        out.push_str("Entities:\n");
        // Decision: the members of a data, type, trait, or enum are written too, as
        // `interfaceOf` writes them, since a dependent relies on those as much as on the
        // declaration itself.
        for &entity in &interface.entities {
            write_interface_entity(&mut out, model, entity, "");
            for member in members_of(model, entity) {
                write_interface_entity(&mut out, model, member, "  ");
            }
        }
        out.push('\n');
    }
    if hosted && !interfaces.is_empty() {
        out.push('\n');
    }

    // The commands.
    // @lfy def/generation/main.lfy:request#request:request:09b0190ee8969e01b9ff7a561a98c3663f87fb8b97fa83c588d0631e7989bc20
    write_commands(&mut out, target, commands);

    // One section per unit of the batch, in order.
    // @lfy def/generation/main.lfy:request#request:request:d86c17626a07f5e8ec807bd7740a5f6f8280e6af67444bf1bf5d18744b801d3b
    out.push_str("## The units to compile\n\n");
    for &index in &batch.units {
        let unit = &plan.units[index];
        let file = &workspace.files[unit.file];
        let lowered = &program.files[unit.lowered];
        let answered = answered.get(&file.path).map(Vec::as_slice);
        let rewordings = reworded.get(&file.path).map(Vec::as_slice).unwrap_or(&[]);
        let _ = writeln!(out, "### `{}` (stem `{}`)\n", file.path, unit.stem);
        // A unit whose lowered code is unchanged has criteria or tests that are not: the
        // code is held to what each of them now says, which may be behavior it never had.
        // @lfy def/generation/main.lfy:request#request:request:dab229dc001954e941ede8f9cf711d6ff5830dd88e08a35f3bccea3da26ad09b
        if unit.reason == Some(Reason::Requirements) {
            out.push_str(
                "The lowered code of this unit is unchanged, but its criteria or tests differ. The \
                 code must do what each criterion and test now says, which may mean writing \
                 behavior it does not have yet.\n\n",
            );
        }
        // A criterion or test the existing outputs did not answer for may be a removed one
        // put in other words, and the code is held to the new wording either way. One that
        // rewords a criterion in place is marked reworded rather than new, so it is no part
        // of this.
        // @lfy def/generation/main.lfy:request#request:request:40e519bc14a840e00c6e0fb7bff68febae9b5b591f182632fddc0a29f6f47704
        if answered.is_some_and(|ids| {
            unit.requirements(program).iter().any(|requirement| {
                !ids.iter().any(|id| id == requirement.id())
                    && !rewordings
                        .iter()
                        .any(|rewording| rewording.id == requirement.id())
            })
        }) {
            out.push_str(
                "A criterion or test marked new may restate a removed one in other words. Read the \
                 code against what the new one says, never against the removed one.\n\n",
            );
        }
        // A criterion whose text changed keeps its place and takes a new id, so the markers
        // of its old id have already been moved and only what it now asks is left to read.
        // @lfy def/generation/main.lfy:request#request:request:5dcbf4d56e0e083dd49457b03e35a549d6a72d10894e442c3a9f608dba553186
        if !rewordings.is_empty() {
            out.push_str(
                "A marker naming the id a reworded criterion or test replaces is rewritten to name \
                 its new id, and compile has done so before you run. Read the code of a reworded \
                 one against what it says now, and change it wherever it asks for what the code \
                 does not do.\n\n",
            );
        }
        if let Some(source) = sources.get(&file.path) {
            out.push_str("Lowered source:\n\n```elfie\n");
            out.push_str(source);
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        if unit.entities.is_empty() {
            out.push_str("The unit has no entities.\n\n");
        }
        write_moves(&mut out, workspace, plan, index);
        // Each entity's lowered code, and then, apart from the code, its local criteria and
        // tests, each on its own line with its id. @lfy def/generation/main.lfy:request
        for &entity in &unit.entities {
            write_entity(
                &mut out,
                model,
                program,
                unit.lowered,
                entity,
                answered,
                rewordings,
            );
        }
        // The criteria and tests of the file's own entity belong to no declaration of it.
        // @lfy def/generation/main.lfy:request
        let own: Vec<Requirement> = lowered
            .criteria
            .iter()
            .cloned()
            .map(Requirement::Criterion)
            .chain(lowered.tests.iter().cloned().map(Requirement::Test))
            .collect();
        if !own.is_empty() {
            let _ = writeln!(out, "#### `{}` (file)\n", file.path);
            write_requirements(&mut out, &own, answered, rewordings);
        }
        // What the outputs answered for and the unit no longer has, last in its section: an
        // id and a text are all that is left of a criterion or a test a unit has dropped.
        // @lfy def/generation/main.lfy:request#request:request:11d52a6cfc7c6727d63a4b8f9330ce26ffeb26adddad76ca0ec87d826a89e8b2
        if let Some(gone) = removed.get(&file.path).filter(|gone| !gone.is_empty()) {
            out.push_str("#### Removed since its outputs were accepted\n\n");
            for requirement in gone {
                // The text of a criterion the outputs answered for is not known where the
                // source they were accepted from is not: only its id was recorded.
                // @lfy def/generation/main.lfy:request#request:request:fad31483b1a56330cc1438cb3020ce5c42a81a5a92c5eb382ce0929be2df4eaa
                let said = if requirement.text.is_empty() {
                    "what it said is not known"
                } else {
                    &requirement.text
                };
                let _ = writeln!(out, "- `{}` — {said}", requirement.id);
            }
            out.push('\n');
        }
    }

    // Every global criterion and test, each once with its id.
    // @lfy def/generation/main.lfy:request#request:request:947030aa711f453c710b970e654a978a41f4db5a2776707bc669e1c0efd5bea8
    out.push_str("## Global criteria and tests\n\n");
    out.push_str(
        "These hold across the whole program. A unit answers for one only where it is relevant to \
         that unit's code, and ignores it otherwise; the global review checks each one once, \
         across every output.\n\n",
    );
    if globals.is_empty() {
        out.push_str("(none)\n\n");
    } else {
        write_requirements(&mut out, globals, None, &[]);
        out.push('\n');
    }

    // Criteria and tests are never restated in an output.
    // @lfy def/generation/main.lfy:request#request:request:88bad81c76d679b95e82ea566e65627b4a1615584da2fe8b645fd322f0b26fb3
    out.push_str("## Criteria and tests in an output\n\n");
    out.push_str(
        "Never restate a criterion or a test in an output, as a comment or as documentation. An \
         output names one only by its id, in the marker of the region that answers for it.\n\n",
    );

    // The standard library. @lfy def/generation/main.lfy:request
    out.push_str("## The standard library\n\n");
    out.push_str(
        "The data and fns of the standard library that carry `builtin` are bound to what the guidance \
         names for them, and are never generated: they are only called. A member of the standard \
         library written out in full is translated where it is used.\n\n",
    );

    // The rules for kinds. @lfy def/generation/main.lfy:request
    out.push_str("## Rules for kinds\n\n");
    out.push_str(
        "- A DataDeclaration becomes a type.\n\
         - An AgentFunctionDeclaration becomes a function whose body satisfies every criterion and test.\n\
         - A FunctionDeclaration is translated statement by statement with nothing added.\n\
         - A TraitDeclaration emits nothing of its own; its members and criteria belong to each entity that carries it.\n\
         - A TypeDeclaration and an EnumDeclaration become their nearest equivalents.\n\
         - An ExternalDeclaration is bound to what the guidance says.\n\
         - A folded value is written as the literal it holds, and a prompted value, spelled \
           `Type@like(`the prompt`)`, is chosen to fit its prompt.\n\
         - A Use is satisfied by the interfaces.\n\n",
    );

    // Markers.
    // @lfy def/generation/main.lfy:request#request:request:671617e25d98e36f8c646b0c76d941034839245f2544e239cb4ede3581bb2f33
    // @lfy def/generation/main.lfy:request#request:request:44f9eb2a9dcf558687d93ade60ce6a62697e5ff6c388a512e83449be94e9a7fd
    out.push_str("## Markers\n\n");
    // The marker comment is spelled out, since the line comment a marker is written in must
    // begin with it and nothing else in the request says how this language begins one.
    let _ = write!(
        out,
        "Every emitted item and every test carries a marker naming the entity it comes from. A marker is \
         written as a line comment beginning with `{}`, then `@lfy`, a space, the source path \
         relative to the root, a colon, and the name of the entity: its identifier, or its owner's \
         identifier, a dot, and a member's name. Never write a line number. A region that answers \
         for a criterion or a test spells its id after the entity and a `#`. Write a criterion's id \
         only on code that does what that criterion says in every situation it names: a marker \
         never stands in for the code, and a marker on code that does something else is a false \
         claim about the program.\n\n",
        target.marker_comment
    );
    for &index in &batch.units {
        let unit = &plan.units[index];
        let file = &workspace.files[unit.file];
        if let Some(&entity) = unit.entities.first() {
            let _ = writeln!(
                out,
                "For example: `{} @lfy {}:{}`.",
                target.marker_comment,
                file.path,
                entity_name(model, entity)
            );
        }
    }
    out.push('\n');

    // Tests.
    // @lfy def/generation/main.lfy:request#request:request:6151dc423cf3f97336663532752b0e5607981574be689ec41445917c49163e8d
    out.push_str("## Tests\n\n");
    out.push_str(
        "Each test becomes one test, and each criterion that can be checked mechanically becomes one \
         test, placed where the guidance says. Each of them carries a marker naming the entity it \
         comes from and, after a `#`, the id of the criterion or test it answers for.\n\n",
    );

    // The tools of the agent server.
    // @lfy def/generation/main.lfy:request#request:request:43974a3eb5968199e4c0d2b5b84afd218c864eb58c47c072bbcc471866a03921
    out.push_str("## The agent server\n\n");
    out.push_str(
        "The tools of the agent server are called before files are searched or read by hand, because \
         they answer from the program and its source maps at once:\n\n\
         - `elfie_entity` for a definition with its criteria and tests.\n\
         - `elfie_find` for where a name is declared.\n\
         - `elfie_references` for where it is used.\n\
         - `elfie_outline` for the declarations of a file.\n\
         - `elfie_output` for the generated code of an entity, of a requirement id, or of an output \
           file, and, with `text` false, for the regions of an output file without their code.\n\
         - `elfie_search` for the lines of the outputs that match a pattern.\n\
         - `elfie_source` for where a line of generated code came from.\n\
         - `elfie_changes` for what changed in a unit since its outputs were accepted.\n\
         - `elfie_check` for whether acceptance would pass.\n\n",
    );
    if hosted {
        // @lfy def/generation/main.lfy:request#request:request:d7b94bd7ef8593cddf59f563c5936407b8173a96ce3ad639afb1cdab7cd6f327
        out.push_str(
            "This session offers these too:\n\n\
             - `elfie_task` gives the obligations of this batch again: each unit's outputs and why it \
               is planned, and every criterion and test with its id, its place, and its text, marked \
               new, kept, or unknown, with the ones removed since the outputs were accepted.\n\
             - `elfie_mark` writes a marker line, and rewrites the id every marker of this batch's \
               outputs names, so that you never type an id by hand.\n\n",
        );
    }

    // How to end the report.
    // @lfy def/generation/main.lfy:request#request:request:e4ff528873311ac2c054d3eb98c7015eca5064897eae15ff47429cfdd53b7860
    out.push_str("## How to report\n\n");
    if hosted {
        // @lfy def/generation/main.lfy:request#request:request:4a237f69a0cf92b366172dde838e3583135a0dc294c8ef43f2392d08c1c1e1bd
        out.push_str(
            "Submit the outcome through `elfie_submit` before your final message: `done` when every \
             output is written, `blocked` with the reason as its message, or `clarify` with the \
             question as its message. Your final message still ends with the line that outcome \
             reads.\n\n",
        );
    }
    out.push_str(
        "End your report with exactly one line:\n\n\
         - `ELFIE: DONE` when every output is written.\n\
         - `ELFIE: BLOCKED: ` followed by the reason when an ambiguous criterion, two criteria in \
           conflict, a name that resolves nowhere, or anything else stops you; write no output for that \
           unit and quote the criterion.\n\
         - `ELFIE: CLARIFY: ` followed by one question when only a person can decide; write nothing.\n",
    );

    out
}

/// What the outputs part says about outputs that exist already: which they are, that only
/// the difference between the previous lowered source and the current one, the criteria
/// and tests marked new, reworded, or removed, and a split the target's guidance requires call
/// for a change, that names and structure are otherwise kept, the previous lowered source of
/// each unit that has one, which units have none, and which units have no record of the ids
/// their outputs were generated against.
// @lfy def/generation/main.lfy:request#request:request:ce2766fa22ba976093fabd91c70450cb8ddf39a9d8bd6e5084875bc916c84425
fn write_existing(
    out: &mut String,
    workspace: &Workspace,
    plan: &Plan,
    batch: &Batch,
    quoted: &Quoted<'_>,
) {
    if quoted.existing.is_empty() {
        return;
    }
    out.push_str("These outputs exist already:\n");
    for path in quoted.existing.keys() {
        let _ = writeln!(out, "- `{path}`");
    }
    out.push_str(
        "\nChange only what the difference between the previous source and the current source, the \
         criteria and tests marked new, reworded, or removed, and a split the target's guidance \
         requires, require. Keep names and structure, except that a module the target's guidance \
         says to split gains submodule files and has its code moved into them: that split is the \
         one change of structure allowed, and it keeps every name other units use. Keep the \
         markers of unchanged items.\n",
    );
    let paths: Vec<&String> = batch
        .units
        .iter()
        .map(|&index| &workspace.files[plan.units[index].file].path)
        .collect();
    for path in &paths {
        if let Some(text) = quoted.previous.get(*path) {
            let _ = writeln!(
                out,
                "\nThe previous source of `{path}`, lowered as its current source is:\n\n\
                 ```elfie\n{}\n```",
                text.trim_end()
            );
        }
    }
    // A unit whose outputs record no source is reconciled with its source instead.
    // @lfy def/generation/main.lfy:request#request:request:3c1f4cba72285105fde5be93378fbda2d3d93aaf4293fbe643ab3a97d7d1b779
    let unknown: Vec<&&String> = paths
        .iter()
        .filter(|path| !quoted.previous.contains_key(**path))
        .collect();
    if !unknown.is_empty() {
        out.push_str(
            "\nThe source these outputs were generated from is not known. Reconcile the existing \
             outputs of each of them with its source:\n",
        );
        for path in unknown {
            let _ = writeln!(out, "- `{path}`");
        }
    }
    // A unit whose outputs record no ids leaves nothing to say which of its criteria and
    // tests are new, so the existing code is read against every one of them.
    // @lfy def/generation/main.lfy:request#request:request:a960df7e08bfeb90c2fb135fc71e6e80e01a69f9f351d3d8c7eab08f069407c1
    let unanswered: Vec<&&String> = paths
        .iter()
        .filter(|path| !quoted.answered.contains_key(**path))
        .collect();
    if !unanswered.is_empty() {
        out.push_str(
            "\nWhich criteria and tests of these units are new is not known. Read the existing code \
             against each of them before you write its marker:\n",
        );
        for path in unanswered {
            let _ = writeln!(out, "- `{path}`");
        }
    }
    out.push('\n');
}

/// What the compiler is given to read, in the order the request gathered it: a reference
/// or an example either quoted in full or named to be read; a definition as the files its
/// use path resolves to; a tool as the tool of the agent server to call.
// @lfy def/generation/main.lfy:request#request:request:3d9e3582187fd1f1532a46bf0f938c787311c81feb59f4a7d7d5b0da9a813a9a
fn write_knowledge(out: &mut String, workspace: &Workspace, knowledge: &[Knowledge]) {
    out.push_str("## Knowledge\n\n");
    if knowledge.is_empty() {
        out.push_str("The compiler is given nothing of its own to read.\n\n");
        return;
    }
    out.push_str("The compiler is given this to read, because it may not already know it:\n\n");
    let mut quoted: Vec<(String, String, String)> = Vec::new();
    for item in knowledge {
        match item.kind {
            // @lfy def/generation/main.lfy:request#request:request:18fa49f6ae0fe059f4d8b65429f7356e822363b0bb901d611f266ca1393c940b
            KnowledgeKind::Tool => {
                let _ = writeln!(
                    out,
                    "- {}: call the tool `{}` of the agent server before relying on what it \
                     answers.",
                    item.topic, item.source
                );
            }
            // @lfy def/generation/main.lfy:request#request:request:179394ea2be57b4a87b41dac2988b30f36b058129a701fc79ff34cc290769606
            KnowledgeKind::Definition => {
                let files = definition_files(workspace, item);
                let named = if files.is_empty() {
                    format!("`{}`", item.source)
                } else {
                    files
                        .iter()
                        .map(|path| format!("`{path}`"))
                        .collect::<Vec<String>>()
                        .join(", ")
                };
                let _ = writeln!(out, "- {}: read {named} as Elfie.", item.topic);
            }
            KnowledgeKind::Reference | KnowledgeKind::Example => {
                let path = under_root(&knowledge_root(workspace, item), &item.source);
                if item.quote == Some(true) {
                    // @lfy def/generation/main.lfy:request#request:request:f64fa963c69a9e30aa2c190c595acdbb6221274d050a5a8533b9cf9291684155
                    let text = std::fs::read_to_string(workspace.root.join(&path))
                        .unwrap_or_else(|error| format!("(`{path}` could not be read: {error})"));
                    quoted.push((item.topic.clone(), path, text));
                } else {
                    // @lfy def/generation/main.lfy:request#request:request:737e4198f2f6bddc790512ae099b13648ed59405ba19506fb0abff20748ec530
                    let _ = writeln!(
                        out,
                        "- {}: read `{path}` before writing anything it covers.",
                        item.topic
                    );
                }
            }
        }
    }
    out.push('\n');
    for (topic, path, text) in quoted {
        let _ = writeln!(
            out,
            "### {topic}\n\nFrom `{path}`:\n\n````\n{}\n````\n",
            text.trim_end()
        );
    }
}

/// The directory a knowledge item's source is relative to: the root of the package of the
/// file its contributor is declared in, and the project's own root when that file is the
/// project's own.
// @lfy def/generation/main.lfy:request
fn knowledge_root(workspace: &Workspace, item: &Knowledge) -> String {
    workspace.model.entities[item.contributor]
        .file
        .and_then(|source| workspace.files.iter().find(|file| file.source == source))
        .and_then(|file| file.package)
        .map_or(String::new(), |package| {
            workspace.packages[package].root.clone()
        })
}

/// The files of the program the source of a knowledge item of the kind `definition`
/// resolves to: it is a use path, read from the file its contributor is declared in.
// @lfy def/generation/main.lfy:request
fn definition_files(workspace: &Workspace, item: &Knowledge) -> Vec<String> {
    let from = workspace.model.entities[item.contributor]
        .file
        .and_then(|source| workspace.files.iter().find(|file| file.source == source));
    let base = if item.source.starts_with('.') {
        let directory = from.map_or("", |file| directory_of(&file.path));
        normalized(&under_root(directory, &item.source))
    } else {
        let (first, rest) = item
            .source
            .split_once('/')
            .unwrap_or((item.source.as_str(), ""));
        match workspace
            .packages
            .iter()
            .find(|package| package.identifier == first)
        {
            Some(package) => normalized(&under_root(&package.root, rest)),
            None => normalized(&item.source),
        }
    };
    let named = format!("{base}{EXTENSION}");
    let main = under_root(&base, &format!("main{EXTENSION}"));
    workspace
        .files
        .iter()
        .filter(|file| file.path == named || file.path == main)
        .map(|file| file.path.clone())
        .collect()
}

/// The commands the compiler runs instead of knowing the tools, one line per operation.
// @lfy def/generation/main.lfy:request
fn write_commands(out: &mut String, target: &Target, commands: &[Command]) {
    out.push_str("## Commands\n\n");
    let _ = writeln!(
        out,
        "These run with `sh -c` in `{}`, with `RUN_SCRIPT` set to {} and, for `add`, `NAME` and \
         `VERSION` set to the dependency's:\n",
        target.output_directory,
        match &target.script_runner {
            Some(runner) => format!("`{runner}`"),
            None => "nothing, since the target names none".to_string(),
        }
    );
    if commands.is_empty() {
        out.push_str("(none)\n");
    }
    for command in commands {
        let _ = writeln!(
            out,
            "- `{}`: {}",
            operation_name(command.operation),
            match &command.line {
                Some(line) => format!("`{line}`"),
                None => "not run".to_string(),
            }
        );
    }
    out.push_str(
        "\nA dependency is added only by running the add command, never by editing a manifest or a \
         lockfile by hand. The build, test, and lint commands are run before the report, and their \
         results stated in it.\n\n",
    );
    // The command of an operation the agent server has a tool for is run through that tool,
    // which runs the same line; a line is run by hand only where no tool is offered for it.
    // @lfy def/generation/main.lfy:request#request:request:f700bbb14b11b81fadfe6c18b018250b1c7b9cdadbd0bd8b2a66ce896dfc54c9
    out.push_str(
        "Where the agent server offers `elfie_build`, `elfie_test`, `elfie_lint`, or `elfie_format`, \
         the command of that operation is run through that tool. Run a line by hand only for an \
         operation whose tool is not offered.\n\n",
    );
    // @lfy def/generation/main.lfy:request#request:request:f4b1430a5e2784e149775d2e69bdf4d652782d426521aa999f67d64b6c8a76c5
    out.push_str(
        "The filter of `elfie_test` is given to the test command as `TESTS`, and the scope of \
         `elfie_build`, `elfie_test`, and `elfie_lint` is given to the command as `SCOPE`. Either \
         narrows the run only where the command's line reads it.\n\n",
    );
}

/// The name of an operation, as the standard library spells it.
// @lfy def/generation/main.lfy:request
fn operation_name(operation: Operation) -> &'static str {
    match operation {
        Operation::Install => "install",
        Operation::Add => "add",
        Operation::Build => "build",
        Operation::Test => "test",
        Operation::Lint => "lint",
        Operation::Format => "format",
        Operation::Run => "run",
    }
}

/// What moved into and out of one unit, in that unit's section of the instructions: which
/// entities, from or into which unit, and because of which cause.
// @lfy def/generation/main.lfy:request
fn write_moves(out: &mut String, workspace: &Workspace, plan: &Plan, index: usize) {
    let model = &workspace.model;
    let spelled = |entities: &[EntityId]| -> String {
        entities
            .iter()
            .map(|&entity| format!("`{}`", entity_name(model, entity)))
            .collect::<Vec<String>>()
            .join(", ")
    };
    // @lfy def/generation/main.lfy:request
    let into: Vec<&Move> = plan
        .moves
        .iter()
        .filter(|moved| moved.into == index)
        .collect();
    if !into.is_empty() {
        // @lfy def/generation/main.lfy:request#request:request:2a957e8526f8fadbdb509afff57d5ee421e83c1bf0bc08f037202c40650af324
        // @lfy def/generation/main.lfy:request#request:request:69a7dba9df720e986cae645ffafd7885bc8c7022024d385145cf81e43867eea5
        out.push_str(
            "These entities are compiled in this unit although another file declares them, because \
             the two files would otherwise reach each other. Write them into this unit's outputs \
             with markers naming the file that declares them. Wherever this unit's code needs only \
             them, a `use` of that file is satisfied by them here and never by importing that \
             file.\n",
        );
        // @lfy def/generation/main.lfy:request#request:request:78a8534645507634a61935dcee1699d73fdc5735bcadda06eb24bf9f9854ea34
        for moved in into {
            let origin = &plan.units[moved.origin];
            let _ = writeln!(
                out,
                "- {} from `{}`, because of `{}`.",
                spelled(&moved.entities),
                workspace.files[origin.file].path,
                moved.cause
            );
        }
        out.push('\n');
    }
    // @lfy def/generation/main.lfy:request
    let outward: Vec<&Move> = plan
        .moves
        .iter()
        .filter(|moved| moved.origin == index)
        .collect();
    if !outward.is_empty() {
        // @lfy def/generation/main.lfy:request#request:request:1672e75b3730e67adb65f5507e3af45281e70066521a13fafb939415a73393da
        // @lfy def/generation/main.lfy:request#request:request:ab56007e0a1646c5e4d399cc67e5ac36f77f95f1ed5af06ced3edda5f7506ec4
        out.push_str(
            "These entities this file declares are compiled in another unit, because the two files \
             would otherwise reach each other. Import them from that unit's outputs and never write \
             them again, and make each of them available from this unit's outputs under the name \
             and path it would have had, so that code naming it here still finds it.\n",
        );
        // @lfy def/generation/main.lfy:request#request:request:d8077ccebe94afe4cb2e7e3cf3daafbef0270bf0ca0b6141d42e3be46d002c52
        for moved in outward {
            let into = &plan.units[moved.into];
            let _ = writeln!(
                out,
                "- {} in the unit `{}` (`{}`).",
                spelled(&moved.entities),
                into.stem,
                workspace.files[into.file].path
            );
        }
        out.push('\n');
    }
}

/// One entity of a unit: its name, its kind, its definition, its type, its lowered code,
/// and then, apart from the code, its local criteria and tests, each on its own line with
/// its id.
// @lfy def/generation/main.lfy:request
fn write_entity(
    out: &mut String,
    model: &Model,
    program: &Program,
    lowered: usize,
    entity: EntityId,
    answered: Option<&[String]>,
    reworded: &[Rewording],
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
    if let Some(ty) = &record.ty {
        let _ = writeln!(out, "Type: `{}`\n", model::type_text(model, ty));
    }
    // An entity moved into the unit is declared in the file it came from, so its code is
    // looked for across the program when the unit's own lowered file does not hold it.
    // @lfy def/generation/main.lfy:request
    if let Some(node) = lowered_node_of(&program.files[lowered], entity)
        .or_else(|| lowered_declaration(program, entity))
    {
        out.push_str("Lowered code:\n\n```elfie\n");
        out.push_str(lowered_code(model, node).trim());
        out.push_str("\n```\n\n");
    }
    write_requirements(
        out,
        &declared_requirements(program, lowered, entity),
        answered,
        reworded,
    );
}

/// Every criterion and test of a list, criteria then tests, each on one line beginning with
/// its id: what a request and a review quote apart from the code.
///
/// Where `answered` holds the ids the existing outputs were generated against, a line whose
/// id is none of them is marked new, so that a compiler reading existing code knows which of
/// them that code was never held to. A line whose id rewords one of them is marked reworded
/// instead, and the id and the text it replaces follow it on a line of their own.
// @lfy def/generation/main.lfy:request#request:request:72801863f22ee154bf0e22299d1405c97cd6309fc630197aa315508e2f4829f7
// @lfy def/generation/main.lfy:request#request:request:d2d5d86c8b26bed5bb851853987bcc7fdd4050b23289d7c04705420520afdb99
fn write_requirements(
    out: &mut String,
    requirements: &[Requirement],
    answered: Option<&[String]>,
    reworded: &[Rewording],
) {
    let rewording = |id: &str| -> Option<&Rewording> {
        reworded.iter().find(|rewording| rewording.id == id)
    };
    let mark = |id: &str| -> &'static str {
        if rewording(id).is_some() {
            return " (reworded)";
        }
        match answered {
            Some(ids) if !ids.iter().any(|known| known == id) => " (new)",
            _ => "",
        }
    };
    // What a reworded one replaces, on the line after it.
    // @lfy def/generation/main.lfy:request#request:request:e1d45d2b90e4f031fa2c8711633c2ef9ff914b2ca15467eee998d20246c6a3d9
    let replaced = |out: &mut String, id: &str| {
        if let Some(rewording) = rewording(id) {
            let _ = writeln!(out, "  replaces {} — {}", rewording.replaces, rewording.text);
        }
    };
    let criteria: Vec<&LoweredCriterion> = requirements
        .iter()
        .filter_map(|requirement| match requirement {
            Requirement::Criterion(criterion) => Some(criterion),
            Requirement::Test(_) => None,
        })
        .collect();
    let tests: Vec<&LoweredTest> = requirements
        .iter()
        .filter_map(|requirement| match requirement {
            Requirement::Test(test) => Some(test),
            Requirement::Criterion(_) => None,
        })
        .collect();
    out.push_str("Criteria:\n");
    if criteria.is_empty() {
        out.push_str("(none)\n");
    }
    for criterion in criteria {
        let _ = writeln!(
            out,
            "- `{}` — {}{}",
            criterion.id,
            criterion.text(),
            mark(&criterion.id)
        );
        replaced(out, &criterion.id);
    }
    out.push('\n');
    out.push_str("Tests:\n");
    if tests.is_empty() {
        out.push_str("(none)\n");
    }
    for test in tests {
        let _ = writeln!(
            out,
            "- `{}` — {}{}",
            test.id,
            test_text(test),
            mark(&test.id)
        );
        replaced(out, &test.id);
    }
    out.push('\n');
}

/// One criterion of the guidance as a list item: `When` and its situations, then its
/// behaviors, then its side effects.
///
/// A criterion of the guidance is a criterion as written, never lowered, so it has no
/// [`LoweredCriterion::text`] to be shown by; it is spelled here exactly as that spells a
/// lowered one, so that every criterion a request quotes reads the same way.
// @lfy def/generation/main.lfy:request
fn write_criterion(out: &mut String, criterion: &Criterion) {
    let mut line = String::new();
    if let Some(situation) = &criterion.situation {
        line.push_str("When ");
        line.push_str(&situation.join(", and "));
        line.push_str(": ");
    }
    if let Some(behavior) = &criterion.behavior {
        line.push_str(&behavior.join("; "));
    }
    if let Some(side_effects) = &criterion.side_effects {
        line.push_str(". Side effects: ");
        line.push_str(&side_effects.join("; "));
    }
    let _ = writeln!(out, "- {line}");
}

/// The local criteria and tests of one entity of a unit, criteria first: those of its
/// declaration in the unit's own lowered file, or, for an entity moved into the unit, those
/// of its declaration in the file it came from.
// @lfy def/generation/main.lfy:request
// @lfy def/generation/main.lfy:review
fn declared_requirements(program: &Program, lowered: usize, entity: EntityId) -> Vec<Requirement> {
    let own = entity_requirements(&program.files[lowered], entity);
    if !own.is_empty() || lowered_node_of(&program.files[lowered], entity).is_some() {
        return own;
    }
    let Some(node) = lowered_declaration(program, entity) else {
        return Vec::new();
    };
    node.criteria
        .iter()
        .cloned()
        .map(Requirement::Criterion)
        .chain(node.tests.iter().cloned().map(Requirement::Test))
        .collect()
}

/// One file as it was when the outputs generated from it were accepted: its lowered text,
/// and each of its local criteria and tests as the id, the place, and the text a
/// [`RemovedRequirement`] holds, in the order it gives them.
///
/// The text as written is bound through [`change`](crate::workspace::change) and lowered, so
/// the text that comes back is the lowered text a request carries for the file now and the
/// two can be read side by side, and the ids come out spelled as the unit's own are. The
/// place and the text of each criterion and test are taken here rather than kept as a node
/// to read later, because the origin of a criterion belongs to this lowering, which is gone
/// once the text has been read off it. `None` when the workspace holds no file at that path.
// @lfy def/generation/main.lfy:request#request:request:e6a9a7f8f68b31456fa6f45f2971df1eddc6eed03b9bcb155872fff58c0ca957
// @lfy def/generation/main.lfy:request#request:request:1b7315cf290fcd29e9e15f54bd98742f52b2943246c4cb0414ed750188b9e3da
fn file_as_it_was(
    program: &Program,
    path: &str,
    text: &str,
) -> Option<(String, Vec<RemovedRequirement>)> {
    let before = crate::interpret::lower(crate::workspace::change(
        &program.workspace,
        path,
        Some(text),
    ));
    let index = before
        .workspace
        .files
        .iter()
        .position(|file| file.path == path)?;
    let file = &before.files[index];
    let model = &before.workspace.model;
    let requirements = local_requirements(file)
        .iter()
        .map(|requirement| match requirement {
            Requirement::Criterion(criterion) => RemovedRequirement {
                id: criterion.id.clone(),
                place: origin_place(model, criterion.origin),
                text: criterion.text(),
            },
            Requirement::Test(test) => RemovedRequirement {
                id: test.id.clone(),
                place: origin_place(model, test.origin),
                text: test_text(test),
            },
        })
        .collect();
    Some((file.text.clone(), requirements))
}
