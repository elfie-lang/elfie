// @lfy def/generation/request.lfy:request
//! Everything the compiler is handed to produce the outputs of one unit.

use super::*;

/// Everything the compiler is handed to produce the outputs of one unit.
///
/// The source is the lowered text of the unit's file, and the requirements its local
/// criteria and tests, each with its id; the globals are every global criterion and test of
/// the program; `existing` is kept only where its path is among the unit's outputs; there is
/// one interface per dependency of the unit; the guidance is every criterion of the target's
/// declaration, of each of its layers, and of every trait those layers extend, as
/// [`guidance_for`] gathers them; the native dependencies and the commands are the target's;
/// and the knowledge is the target's, then that of each entity of the unit, then that of
/// each entity of an interface, each item once. The instructions quote every criterion and
/// test already resolved, so a compiler with no access to the model can still work.
///
/// The instructions put first what is the same for every unit of the target — the role, the
/// guidance, the native dependencies, the target's knowledge, the commands, the global
/// criteria and tests, the rules, the stubs, the tools, and the report — and only then what
/// is the unit's own, so that a prompt cache holds the first part across units.
///
/// What was dropped and what was reworded are kept apart: a criterion that kept its place
/// and took a new id is a rewording, with the id and the text it replaces, while one the
/// unit no longer has at all is removed. A unit nothing else differs for has
/// `reworded_only` set, so a caller may move its markers and ask no compiler at all.
///
/// `unit` is the unit's index in [`Plan::units`]. `previous` is the unit's source at its last
/// accepted generation, or its hash-matched source, recovered and lowered once for the
/// compile, and `None` when not known; the file as it was is read from its program, and
/// nothing is lowered or bound here. `hosted` is whether the compiler reading the request
/// runs in a compiler session of the compile-hosted agent server, which offers `elfie_task`,
/// `elfie_dependency`, `elfie_mark`, `elfie_reach`, `elfie_touch`, and `elfie_submit`.
///
/// Where [`Previous::hash_matched`] is set, `previous` lowers to the same code and says
/// nothing about what the outputs answered for, so nothing of it is read as a rewording,
/// what was dropped is read off the recorded ids alone — with neither a place nor a text,
/// which that revision does not hold — and the unit is never one whose markers are the whole
/// of the work.
///
/// `foreign_edits` are the edits the change set records since the unit was last accepted in
/// it; only those the unit owns are kept. `relocations` are the relocations of this compile;
/// only those of the unit's target into or out of its file are kept, and a criterion or test
/// of an entity relocated out of the unit is never read as removed from it.
// Decision: the definition passes the plan and the unit; a plan here does not own its
// workspace or the program lowered from it, so the program is an extra first parameter, and
// the unit is named by its index in `Plan::units`, as `Request::unit` names it.
// Decision: the caller says whether the request is for a hosted compiler session, because
// only the caller knows; a hosted compiler pulls an interface when it needs one, so its
// request names the interfaces instead of quoting them, while a request read anywhere else
// quotes them whole. A compiler command that never uses the session it is hosted in still
// gets the interfaces by name only, and reads each definition another way.
// Decision: the request is handed the foreign edits rather than reading them, because only
// the compile loop keeps the change set they are recorded in.
// @lfy def/generation/request.lfy:request
#[allow(clippy::too_many_arguments)]
pub fn request(
    program: &Program,
    plan: &Plan,
    unit: usize,
    existing: &[Output],
    previous: Option<&Previous>,
    hosted: bool,
    foreign_edits: &[ForeignEdit],
    relocations: &[Relocation],
) -> Request {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let index = unit;
    let unit = &plan.units[index];
    let target = &workspace.targets[unit.target];
    let path = &workspace.files[unit.file].path;
    let hash_matched = previous.is_some_and(|previous| previous.hash_matched);

    // @lfy def/generation/request.lfy:request
    let requirements: Vec<Requirement> = unit.requirements(program);
    let held = |id: &str| requirements.iter().any(|requirement| requirement.id() == id);
    // The relocations of the unit's target into or out of the unit's file, in their order.
    // @lfy def/generation/request.lfy:request#request:request:baefb33ae471c0311341c5c3310f02f9c51e8e565478bef7cc9d5d2373b98ed9
    let relocated: Vec<Relocation> = relocations
        .iter()
        .filter(|relocation| relocation.target == target.identifier)
        .filter(|relocation| &relocation.into == path || &relocation.origin == path)
        .cloned()
        .collect();
    // The ids of the local criteria and tests of the entities relocated out of the unit: they
    // are the unit's they moved to now, and were never dropped.
    let relocated_out = relocated_out_ids(program, &relocated, path);
    let kept = |id: &str| held(id) || relocated_out.iter().any(|out| out == id);
    // The unit's file as it was when its outputs were accepted, as the program of `previous`
    // holds it, lowered once already.
    // @lfy def/generation/request.lfy:request#request:request:79b8e2cfc5abf587f5ee588823cd91a2bc93bca07e5e13d1723444029d15b27f
    let was: Option<(String, Vec<RemovedRequirement>)> =
        previous.and_then(|previous| file_as_it_was(previous, path));
    // The lowered text the unit's outputs were generated from, which the instructions read
    // against the lowered text of the unit now.
    // @lfy def/generation/request.lfy:request#request:request:f716d0d680ccdf61f0d7552cc5067d5a325199077fe3d5d7280c889ed2df6c4b
    let previous_lowered: Option<String> = was.as_ref().map(|(lowered, _)| lowered.clone());
    // The ids the existing outputs were generated against: what the map file of the unit
    // recorded, or, where it recorded none, the ids of the file as it was.
    let recorded: Option<Vec<String>> = unit
        .outputs
        .iter()
        .find(|map| !map.answered.is_empty())
        .map(|map| map.answered.clone());
    let answered: Option<Vec<String>> = match recorded {
        // @lfy def/generation/request.lfy:request#request:request:54d1c80b3d1e38ff7f2e58e0cce13959161756838579cc4936f7e34c3efcffab
        Some(ids) => Some(ids),
        // @lfy def/generation/request.lfy:request#request:request:c0f16d7718690de14a23abb41bfc4969ec02c0d005d99bc9c10b079f30ed8856
        None => was.as_ref().map(|(_, before)| {
            before
                .iter()
                .map(|requirement| requirement.id.clone())
                .collect()
        }),
    };
    // Each criterion and test of the unit that rewords one the outputs were generated
    // against: the same place among its entity's criteria or tests, another text, and so
    // another id. What was reworded is not gone, so it is kept apart from what is.
    // @lfy def/generation/data.lfy:Request.reworded
    let rewordings: Option<Vec<Rewording>> = previous.map(|previous| {
        if previous.hash_matched {
            // A hash-matched source is some other revision of the file that lowers to the
            // same code; where its criteria stood says nothing about what the outputs were
            // accepted against, so none of them is read as a rewording.
            // @lfy def/generation/request.lfy:request#request:request:eede918b144fb03a48f104a6350f51f5f036aeb6eb782ff28118a888e72e8fab
            Vec::new()
        } else {
            // @lfy def/generation/request.lfy:request#request:request:1bbc3e20f97e92fee0189ae3e16375c6bd09e15bfead3354e88ceee02de45da1
            reworded(program, unit, previous)
        }
    });
    // Each criterion and test of the file as it was that the unit no longer has and that
    // none of its own rewords, and that no entity relocated out of the unit holds now: what a
    // compiler reading the existing code needs in order to tell code that answers for nothing
    // any more from code that still answers.
    // @lfy def/generation/data.lfy:Request.removed
    let removed: Option<Vec<RemovedRequirement>> = previous.map(|_| {
        if hash_matched {
            // A hash-matched source holds another revision's criteria and tests, so what the
            // outputs answered for is only what their maps recorded: the ids alone, in id
            // order, with neither the place nor the text that revision does not hold.
            // @lfy def/generation/request.lfy:request#request:request:eede918b144fb03a48f104a6350f51f5f036aeb6eb782ff28118a888e72e8fab
            let mut gone: Vec<RemovedRequirement> = answered
                .as_deref()
                .unwrap_or_default()
                .iter()
                .filter(|id| !kept(id))
                .map(|id| RemovedRequirement {
                    id: id.clone(),
                    place: String::new(),
                    text: String::new(),
                })
                .collect();
            gone.sort_by(|left, right| left.id.cmp(&right.id));
            return gone;
        }
        // In the order the file as it was gives them.
        // @lfy def/generation/request.lfy:request#request:request:b2ab29be7b2169db89a7489284493604f2ee49d92690fa210f16b75d5e7c5429
        let rewords = rewordings.as_deref().unwrap_or_default();
        was.as_ref()
            .map(|(_, before)| {
                before
                    .iter()
                    .filter(|requirement| !kept(&requirement.id))
                    .filter(|requirement| {
                        !rewords
                            .iter()
                            .any(|rewording| rewording.replaces == requirement.id)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    });
    // Whether the unit's existing outputs differ from what it asks for now only in the ids of
    // reworded criteria and tests: its lowered code is unchanged, nothing was dropped and
    // nothing added, and every interface it was built against still hashes the same. Moving
    // its markers is then the whole of the work, which the caller may do without the
    // compiler; anywhere any of that does not hold, it is not.
    // @lfy def/generation/data.lfy:Request.rewordedOnly
    // @lfy def/generation/request.lfy:request#request:request:618fbef70b317544c9a741f5feeab781cc4c29251a1c0cf07a08fcd52b4021bf
    // @lfy def/generation/request.lfy:request#request:request:dcfedcbc95ad64785427d7952c0d6e5a4a7223f1d42d34620e4d3fb6a1330099
    let reworded_only = {
        let rewords = rewordings.as_deref().unwrap_or_default();
        let known = answered.as_deref().unwrap_or_default();
        unit.reason == Some(Reason::Requirements)
            && previous.is_some()
            && !hash_matched
            && removed.as_ref().is_some_and(Vec::is_empty)
            && !rewords.is_empty()
            // Every criterion and test of the unit was either answered for under the id it
            // still has or rewords one that was.
            && requirements.iter().all(|requirement| {
                known.iter().any(|id| id == requirement.id())
                    || rewords
                        .iter()
                        .any(|rewording| rewording.id == requirement.id())
            })
            // Every dependency's interface is the one the outputs were generated against, so
            // nothing of what the unit may rely on has moved either.
            && unit.dependencies.iter().all(|&dependency| {
                let dependency = &plan.units[dependency];
                let signature = interface_signature(workspace, dependency);
                let of_path = &workspace.files[dependency.file].path;
                unit.outputs
                    .iter()
                    .all(|map| map.dependencies.get(of_path) == Some(&signature))
            })
    };
    // @lfy def/generation/request.lfy:request
    let globals: Vec<Requirement> = program
        .criteria
        .iter()
        .cloned()
        .map(Requirement::Criterion)
        .chain(program.tests.iter().cloned().map(Requirement::Test))
        .collect();
    // @lfy def/generation/request.lfy:request#request:request:403680d3f5bf2a057752d5d237cbfb6d63463cc8feff3f3e51a12a290e249b3e
    let existing: BTreeMap<String, String> = existing
        .iter()
        .filter(|output| unit.outputs.iter().any(|map| map.output == output.path))
        .map(|output| (output.path.clone(), output.text.clone()))
        .collect();
    // The edits the unit owns, in the order they were made.
    // @lfy def/generation/request.lfy:request#request:request:56b3e8ed8845880a5efc54ee766d63a577e1be64b2614849b89bca97e2254907
    let foreign_edits: Vec<ForeignEdit> = foreign_edits
        .iter()
        .filter(|edit| owns(unit, target, edit))
        .cloned()
        .collect();
    // Every later unit of the target that has outputs, read from the plan's units and their
    // outputs alone, so what is planned changes nothing of it.
    // @lfy def/generation/request.lfy:request#request:request:78998a2cc11aaa848bd3cfc62653454eced252200bec2034871a1e4dc4620542
    let reach = Reach::of(workspace, plan, index);
    // One interface per dependency, each holding every entity of that dependency: what a
    // hosted compiler is given by name is the same interface a request read anywhere else
    // quotes whole, so the request carries no less for being hosted.
    // @lfy def/generation/request.lfy:request
    // @lfy def/generation/request.lfy:request#request:request:5eb8b7bd8df8b28d5af1e781560a38611e0b664872e17a14d9c2e7352a7798cf
    let mut interfaces: Vec<Interface> = Vec::new();
    for &dependency in &unit.dependencies {
        if interfaces
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
    let guidance = guidance_for(model, target);
    // @lfy def/generation/request.lfy:request
    let native_dependencies: Vec<NativeDependency> = target.native_dependencies.clone();
    let commands: Vec<Command> = target.commands.clone();
    // The target's knowledge, then that of each entity of the unit, then that of each entity
    // of an interface, each item once.
    // @lfy def/generation/request.lfy:request
    let mut knowledge: Vec<Knowledge> = Vec::new();
    let hold = |items: &[Knowledge], knowledge: &mut Vec<Knowledge>| {
        for item in items {
            if !knowledge.contains(item) {
                knowledge.push(item.clone());
            }
        }
    };
    hold(&target.knowledge, &mut knowledge);
    for &entity in &unit.entities {
        hold(&model.entities[entity].knowledge, &mut knowledge);
    }
    for interface in &interfaces {
        for &entity in &interface.entities {
            hold(&model.entities[entity].knowledge, &mut knowledge);
        }
    }

    let instructions = instructions(
        program,
        plan,
        index,
        target,
        hosted,
        &Quoted {
            source: &unit.text,
            globals: &globals,
            previous: previous_lowered.as_deref(),
            answered: answered.as_deref(),
            removed: removed.as_deref(),
            reworded: rewordings.as_deref().unwrap_or_default(),
            existing: &existing,
            foreign_edits: &foreign_edits,
            reach: &reach,
            relocated: &relocated,
            interfaces: &interfaces,
            guidance: &guidance,
            native_dependencies: &native_dependencies,
            knowledge: &knowledge,
            commands: &commands,
        },
    );

    Request {
        // @lfy def/generation/request.lfy:request#request:request:959d0fbacdba0aea498605d429f4ef8f02ed09ff514957b1eb5c5d9cd0244bf9
        unit: index,
        instructions,
        source: unit.text.clone(),
        requirements,
        globals,
        previous: previous_lowered,
        answered,
        removed,
        reworded: rewordings,
        reworded_only,
        existing,
        foreign_edits,
        reach,
        relocated,
        interfaces,
        guidance,
        native_dependencies,
        knowledge,
        commands,
    }
}

/// The ids of the local criteria and tests of every entity relocated out of the unit whose
/// file is `path`, and of its members: those the entity holds in the file it was relocated
/// into, as the program declares it now.
// @lfy def/generation/request.lfy:request#request:request:b2ab29be7b2169db89a7489284493604f2ee49d92690fa210f16b75d5e7c5429
// @lfy def/generation/request.lfy:request#request:request:eede918b144fb03a48f104a6350f51f5f036aeb6eb782ff28118a888e72e8fab
fn relocated_out_ids(program: &Program, relocated: &[Relocation], path: &str) -> Vec<String> {
    let model = &program.workspace.model;
    let mut out: Vec<String> = Vec::new();
    for relocation in relocated.iter().filter(|relocation| relocation.origin == path) {
        let Some(index) = program
            .workspace
            .files
            .iter()
            .position(|file| file.path == relocation.into)
        else {
            continue;
        };
        let mut nodes = Vec::new();
        lowered_nodes(&program.files[index].root, &mut nodes);
        for node in nodes {
            let Some(entity) = node.entity else { continue };
            if qualified_name(model, entity) != relocation.entity {
                continue;
            }
            // The entity's node and every node within it, which are its members.
            let mut within = Vec::new();
            lowered_nodes(node, &mut within);
            for inner in within {
                out.extend(inner.criteria.iter().map(|criterion| criterion.id.clone()));
                out.extend(inner.tests.iter().map(|test| test.id.clone()));
            }
        }
    }
    out
}

/// What the instructions quote, gathered before they are written.
// Decision: the definition writes the instructions from the request it is building; the
// parts are grouped here rather than passed one by one.
// @lfy def/generation/request.lfy:request
struct Quoted<'r> {
    source: &'r str,
    globals: &'r [Requirement],
    previous: Option<&'r str>,
    answered: Option<&'r [String]>,
    removed: Option<&'r [RemovedRequirement]>,
    reworded: &'r [Rewording],
    existing: &'r BTreeMap<String, String>,
    foreign_edits: &'r [ForeignEdit],
    reach: &'r Reach,
    relocated: &'r [Relocation],
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
// @lfy def/generation/request.lfy:request#request:request:135e15c4022af725fbc8ead4da685356a7bd57007a779d43dc620fb979b49d4e
pub fn guidance_for(model: &Model, target: &Target) -> Vec<Criterion> {
    let held = model::criteria_of(model, target.declaration);
    // A trait reached through two layers is inherited by the declaration once per path, so
    // what one contributor gave is taken once.
    // @lfy def/generation/request.lfy:request
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
    // @lfy def/generation/request.lfy:request
    let mut layered: Vec<EntityId> = Vec::new();
    for &layer in &target.layers {
        if layered.contains(&layer) {
            continue; // @lfy def/generation/request.lfy:request
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
                continue; // @lfy def/generation/request.lfy:request
            }
            layered.push(applied.entity);
        }
    }
    // Whoever else gave the declaration a criterion comes first: the declaration's own body,
    // or the file that adds one with an add call on the const's acceptanceCriteria.
    // @lfy def/generation/request.lfy:request
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
    // @lfy def/generation/request.lfy:request
    for &entity in &layered {
        out.extend(by(entity));
    }
    out
}

/// The heading of the unit's section, which the parts before it name it by.
// @lfy def/generation/request.lfy:request
const UNIT_SECTION: &str = "The unit to compile";

/// The prompt: what to produce, where, the rules for producing it, and how to report the
/// outcome, in the order the definition lists: first every part that reads the same for
/// every unit of the target, then the interfaces, the unit's knowledge, the unit's section,
/// the foreign edits, the existing outputs, and the reach.
// @lfy def/generation/request.lfy:request#request:request:0efe0565880b5d7bbc83ee22c7e44e71d1599754ad1d82c9ea4fc7fd4bda4282
// @lfy def/generation/request.lfy:request#request:request:eb00a710ef7f9ae5f807e0eee8c047af03c75306a424a1d098caac52a1699eb1
fn instructions(
    program: &Program,
    plan: &Plan,
    index: usize,
    target: &Target,
    hosted: bool,
    quoted: &Quoted<'_>,
) -> String {
    let workspace = &program.workspace;
    let mut out = String::new();
    // Nothing up to the interfaces names the unit, so that part is the same text for every
    // unit of the target with the same `hosted`.
    write_role(&mut out, target);
    write_guidance(&mut out, target, quoted.guidance, quoted.native_dependencies);
    // The target's knowledge, which every unit of the target is given.
    // @lfy def/generation/request.lfy:request#request:request:52ca7fc29e1e450ee23c263741df61d61b239c73a255a810b7d879c5cf38f7ac
    let (of_target, of_unit): (Vec<Knowledge>, Vec<Knowledge>) = quoted
        .knowledge
        .iter()
        .cloned()
        .partition(|item| target.knowledge.contains(item));
    write_knowledge(&mut out, workspace, "Knowledge of the target", &of_target);
    write_commands(&mut out, target, quoted.commands);
    write_globals(&mut out, quoted.globals);
    write_rules(&mut out, target);
    write_stubs(&mut out, workspace, target, quoted.reach);
    write_tools(&mut out, hosted);
    write_report(&mut out, hosted);

    // What is the unit's own.
    write_interfaces(&mut out, workspace, plan, hosted, quoted.interfaces);
    // Every other item of the knowledge, in its order.
    // @lfy def/generation/request.lfy:request#request:request:c98e56391cddb2a9c1906590ced0e660422b844fd0eaf115ecabe5c0d384a131
    write_knowledge(&mut out, workspace, "Knowledge of the unit", &of_unit);
    write_unit(&mut out, program, plan, index, quoted);
    write_foreign_edits(&mut out, target, quoted.foreign_edits);
    write_existing(&mut out, quoted);
    write_reach(&mut out, plan, quoted.reach);
    out
}

/// The reader and its job, and where the outputs go.
// @lfy def/generation/request.lfy:request#request:request:fff6f01c8ce622f973502bc046ee6f471011a0368d5ba7c0c0c0a4f4c94a8e0c
// @lfy def/generation/request.lfy:request#request:request:ebef3352a48540ef0a12abd094f973ab2762f62746edd431a7b3d4d95f8c3780
fn write_role(out: &mut String, target: &Target) {
    let _ = writeln!(out, "# Compiling one unit for the target `{}`\n", target.identifier);
    let _ = writeln!(
        out,
        "You are the compiler for the target `{}`. Your whole job is the outputs of the one unit \
         the section \"{UNIT_SECTION}\" names, and you write nothing outside them, except the \
         edits to later units the part \"The reach of this run\" allows.\n",
        target.identifier
    );
    out.push_str("## Where the outputs go\n\n");
    let _ = writeln!(
        out,
        "The outputs go under `{}`, their paths spelled from the unit's stem as the guidance \
         says.\n",
        target.output_directory
    );
}

/// The guidance and the native dependencies.
// @lfy def/generation/request.lfy:request#request:request:135e15c4022af725fbc8ead4da685356a7bd57007a779d43dc620fb979b49d4e
fn write_guidance(
    out: &mut String,
    target: &Target,
    guidance: &[Criterion],
    native_dependencies: &[NativeDependency],
) {
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
        write_criterion(out, criterion);
    }
    out.push('\n');

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
}

/// Every global criterion and test, each once with its id.
// @lfy def/generation/request.lfy:request#request:request:947030aa711f453c710b970e654a978a41f4db5a2776707bc669e1c0efd5bea8
fn write_globals(out: &mut String, globals: &[Requirement]) {
    out.push_str("## Global criteria and tests\n\n");
    out.push_str(
        "These hold across the whole program. A unit answers for one only where it is relevant to \
         that unit's code, and ignores it otherwise; the global review checks each one once, \
         across every output.\n\n",
    );
    if globals.is_empty() {
        out.push_str("(none)\n\n");
    } else {
        write_requirements(out, globals, None, &[]);
        out.push('\n');
    }
}

/// The rules: criteria and tests are never restated, the standard library, the kinds, the
/// markers, and the tests.
// @lfy def/generation/request.lfy:request
fn write_rules(out: &mut String, target: &Target) {
    // Criteria and tests are never restated in an output.
    // @lfy def/generation/request.lfy:request#request:request:88bad81c76d679b95e82ea566e65627b4a1615584da2fe8b645fd322f0b26fb3
    out.push_str("## Criteria and tests in an output\n\n");
    out.push_str(
        "Never restate a criterion or a test in an output, as a comment or as documentation. An \
         output names one only by its id, in the marker of the region that answers for it.\n\n",
    );

    // The standard library. @lfy def/generation/request.lfy:request
    out.push_str("## The standard library\n\n");
    out.push_str(
        "The data and fns of the standard library that carry `builtin` are bound to what the guidance \
         names for them, and are never generated: they are only called. A member of the standard \
         library written out in full is translated where it is used.\n\n",
    );

    // The rules for kinds. @lfy def/generation/request.lfy:request
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

    // Markers. The marker comment is spelled out, since the line comment a marker is written
    // in must begin with it and nothing else in the request says how this language begins
    // one.
    // @lfy def/generation/request.lfy:request#request:request:671617e25d98e36f8c646b0c76d941034839245f2544e239cb4ede3581bb2f33
    // @lfy def/generation/request.lfy:request#request:request:44f9eb2a9dcf558687d93ade60ce6a62697e5ff6c388a512e83449be94e9a7fd
    out.push_str("## Markers\n\n");
    let _ = write!(
        out,
        "Every emitted item and every test carries a marker naming the entity it comes from. A marker is \
         written as a line comment beginning with `{0}`, then `@lfy`, a space, the source path \
         relative to the root, a colon, and the name of the entity: its identifier, or its owner's \
         identifier, a dot, and a member's name. Never write a line number. A region that answers \
         for a criterion or a test spells its id after the entity and a `#`. Write a criterion's id \
         only on code that does what that criterion says in every situation it names: a marker \
         never stands in for the code, and a marker on code that does something else is a false \
         claim about the program.\n\n\
         For example: `{0} @lfy def/example.lfy:Example`.\n\n",
        target.marker_comment
    );

    // Tests.
    // @lfy def/generation/request.lfy:request#request:request:6151dc423cf3f97336663532752b0e5607981574be689ec41445917c49163e8d
    out.push_str("## Tests\n\n");
    out.push_str(
        "Each test becomes one test, and each criterion that can be checked mechanically becomes one \
         test, placed where the guidance says. Each of them carries a marker naming the entity it \
         comes from and, after a `#`, the id of the criterion or test it answers for.\n\n",
    );
}

/// What a run may do to the code of a later unit: a stub, where the target has a stub line,
/// or a fix within the cap, and nothing to its markers.
// @lfy def/generation/request.lfy:request#request:request:23c0b1c1769d21fd81691a3e8adf218de21358861507f7e3ab4b0a8523d6ceaf
fn write_stubs(out: &mut String, workspace: &Workspace, target: &Target, reach: &Reach) {
    out.push_str("## Stubs and fixes in later units\n\n");
    out.push_str(
        "A later unit's code is edited only where this unit's own change would otherwise stop the \
         target's build or a test from passing, and only inside the regions of a unit the part \
         \"The reach of this run\" lists.\n\n",
    );
    // @lfy def/generation/request.lfy:request#request:request:5367b0ce97e471a1ab266e29356a00115ec74e7fcc5ace4805721d77194a180e
    match stub_line_of(&workspace.model, target) {
        // @lfy def/generation/request.lfy:request#request:request:56cbf2c86de7cfc98b613cf1133778da57a587d2a252e62391857d01fa0dcc1e
        Some(stub_line) => {
            out.push_str(
                "A stub replaces the body of an item inside a later unit's region with the stub \
                 line, and keeps the item's signature. The target's stub line is:\n\n",
            );
            let _ = writeln!(out, "```\n{}\n```\n", stub_line.trim());
            let _ = writeln!(
                out,
                "In it, write {{{{unit}}}} as the stem of the unit the section \"{UNIT_SECTION}\" \
                 names, and {{{{reason}}}} as one line saying why the body was replaced.\n"
            );
        }
        // @lfy def/generation/request.lfy:request#request:request:fa7877d32be5d508227d6d6bf22128a10d3041153bcb8b6aee2e60a4efad0c5b
        None => out.push_str(
            "The target has no stub line, so a later unit's code is edited only by a fix.\n\n",
        ),
    }
    // @lfy def/generation/request.lfy:request#request:request:5a3b2f10f5d312fabcd0e80c04eda116a66569db896d16d3c1df765c201a5b12
    let _ = writeln!(
        out,
        "A fix is any other edit inside a later unit's region. It changes at most {} lines in one \
         region and at most {} lines across the regions of one unit, counting for each region the \
         larger of the lines a line diff removes and the lines it adds.",
        reach.fix_lines, reach.fix_unit_lines
    );
    // @lfy def/generation/request.lfy:request#request:request:71a074ec9746e65337f38a647fd2376bc862cd1a25601b0650f2ffa6c566d5ec
    out.push_str("An edit bigger than a fix may be is written as a stub.\n\n");
    // @lfy def/generation/request.lfy:request#request:request:00813b09739a9f5b779cac994285df76689c777d1752c796d019f6732b636c93
    out.push_str("The markers of a later unit's region are never changed, added, or removed.\n\n");
    // @lfy def/generation/request.lfy:request#request:request:62d883a30a28badaea4f03b2960f52fb3aa9c70f5050d0dbf476d7a7f1acbd15
    out.push_str(
        "Every unit a fix is made in needs a note given through `elfie_touch` with that unit's stem \
         and why it was edited, or the run is rejected.\n\n",
    );
    // @lfy def/generation/request.lfy:request#request:request:e2b687435e285ea5c19b4f0a0d795cc0e9ab80185ea8f399f3b9e959041cdcda
    let _ = writeln!(
        out,
        "A stub line left in the regions of the unit the section \"{UNIT_SECTION}\" names rejects \
         the run.\n"
    );
}

/// The tools of the agent server.
// @lfy def/generation/request.lfy:request#request:request:43974a3eb5968199e4c0d2b5b84afd218c864eb58c47c072bbcc471866a03921
fn write_tools(out: &mut String, hosted: bool) {
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
        // @lfy def/generation/request.lfy:request#request:request:ee02a52de55ba8387ed07c92e9dcbdf5b9c85589d8fdaada91f00910e1406e76
        out.push_str(
            "This session offers these too:\n\n\
             - `elfie_task` gives the unit's obligations again: its outputs and its reason, and every \
               criterion and test with its id, its place, and its text, marked new, kept, or \
               unknown, with the ones removed since the outputs were accepted.\n\
             - `elfie_mark` writes a marker line, and rewrites the id every marker of the unit's \
               outputs names, so that you never type an id by hand.\n\
             - `elfie_reach` gives the units the run may edit, their regions, and what is left of \
               the fix cap.\n\n",
        );
    }
}

/// How to end the report.
// @lfy def/generation/request.lfy:request#request:request:e4ff528873311ac2c054d3eb98c7015eca5064897eae15ff47429cfdd53b7860
fn write_report(out: &mut String, hosted: bool) {
    out.push_str("## How to report\n\n");
    if hosted {
        // @lfy def/generation/request.lfy:request#request:request:4a237f69a0cf92b366172dde838e3583135a0dc294c8ef43f2392d08c1c1e1bd
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
         - `ELFIE: CLARIFY: ` followed by one question when only a person can decide; write nothing.\n\n",
    );
}

/// Each interface: quoted whole, or named for a hosted compiler to pull.
// @lfy def/generation/request.lfy:request
fn write_interfaces(
    out: &mut String,
    workspace: &Workspace,
    plan: &Plan,
    hosted: bool,
    interfaces: &[Interface],
) {
    let model = &workspace.model;
    out.push_str("## Interfaces\n\n");
    if interfaces.is_empty() {
        out.push_str("The unit depends on no other unit.\n\n");
    } else {
        // @lfy def/generation/request.lfy:request#request:request:7424b0fcb62916f23bfb6c1265f3a3f943fb1354558e0d4b0ea4722212bc0c7f
        out.push_str(
            "The unit depends on the units below. Use their entities by the names their outputs \
             spell, and never edit their outputs.\n\n",
        );
        if hosted {
            // A hosted compiler pulls the interface of the one entity it needs, so the
            // request names the entities and spells none of them out.
            // @lfy def/generation/request.lfy:request#request:request:91c07c652cfc9edd8b82e5c1bebc5ef3998ae8e528cbd1623161418170399772
            out.push_str(
                "Read the interface of one of those entities — its kind, its definition, its type, its \
                 parameters, and its output — with `elfie_dependency`.\n\n",
            );
        }
    }
    for interface in interfaces {
        let dependency = &plan.units[interface.unit];
        if hosted {
            // @lfy def/generation/request.lfy:request#request:request:c5448d7c2bb4f497fbdd859f9d7405cd0bdf319c54990d943d33d646b6da87d4
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
        // @lfy def/generation/request.lfy:request#request:request:1530126fe56400a7920d4c5c53eb10a8785a3ed3921d00bc2814a0452615fd57
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
            write_interface_entity(out, model, entity, "");
            for member in members_of(model, entity) {
                write_interface_entity(out, model, member, "  ");
            }
        }
        out.push('\n');
    }
    if hosted && !interfaces.is_empty() {
        out.push('\n');
    }
}

/// The unit's section: its heading, where its outputs go, what it is asked now, and what
/// was removed since its outputs were accepted.
// @lfy def/generation/request.lfy:request
fn write_unit(out: &mut String, program: &Program, plan: &Plan, index: usize, quoted: &Quoted<'_>) {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let unit = &plan.units[index];
    let file = &workspace.files[unit.file];
    let lowered = &program.files[unit.lowered];
    let answered = quoted.answered;
    let rewordings = quoted.reworded;

    // @lfy def/generation/request.lfy:request#request:request:61e677b44b6f0c053f64e20b7685d271519f83515df009d807762ca548cbfbeb
    let _ = write!(out, "## {UNIT_SECTION}: `{}` (`{}`)", unit.stem, file.path);
    if let Some(reason) = unit.reason {
        let _ = write!(out, ", planned because it is {reason}");
    }
    out.push_str("\n\n");
    if unit.outputs.is_empty() {
        // @lfy def/generation/request.lfy:request#request:request:f2a22f45bc9513dedff93a88f20ba324a34cd2dbadc5b117686df4eb14f73a47
        out.push_str("Nothing has been generated for it yet.\n\n");
    } else {
        // @lfy def/generation/request.lfy:request#request:request:09f94a5ded65108d10253c1fb3262ce6174fd78e63b564b55a231c913cd36a02
        let paths: Vec<String> = unit
            .outputs
            .iter()
            .map(|map| format!("`{}`", map.output))
            .collect();
        let _ = writeln!(
            out,
            "Its outputs go at {}, except that a module the target's guidance says to split gains \
             submodule files and has its code moved into them.\n",
            paths.join(", ")
        );
    }
    // A unit whose lowered code is unchanged has criteria or tests that are not: the code is
    // held to what each of them now says, which may be behavior it never had.
    // @lfy def/generation/request.lfy:request#request:request:e9d469e6b5e3e7503987163405e59232553f60e223ea220e7bd04734d2dcfd4c
    if unit.reason == Some(Reason::Requirements) {
        out.push_str(
            "The lowered code of this unit is unchanged, but its criteria or tests differ. The \
             code must do what each criterion and test now says, which may mean writing \
             behavior it does not have yet.\n\n",
        );
    }
    // A criterion or test the existing outputs did not answer for may be a removed one put in
    // other words, and the code is held to the new wording either way. One that rewords a
    // criterion in place is marked reworded rather than new, so it is no part of this.
    // @lfy def/generation/request.lfy:request#request:request:1804b6b498c5043da3a7cf8c5a175a457bf933f4f3f4586491df94745de5e1d9
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
    // A criterion whose text changed keeps its place and takes a new id, so the markers of its
    // old id have already been moved and only what it now asks is left to read.
    // @lfy def/generation/request.lfy:request#request:request:3acaed2ffa70c62f8ca37a64447e578819b1c7a7e2d0b480cd8f741b827c3a7e
    if !rewordings.is_empty() {
        out.push_str(
            "A marker naming the id a reworded criterion or test replaces is rewritten to name \
             its new id, and compile has done so before you run. Read the code of a reworded \
             one against what it says now, and change it wherever it asks for what the code \
             does not do.\n\n",
        );
    }
    out.push_str("Lowered source:\n\n```elfie\n");
    out.push_str(quoted.source);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("```\n\n");
    if unit.entities.is_empty() {
        out.push_str("The unit has no entities.\n\n");
    }
    write_moves(out, workspace, plan, index);
    write_relocations(out, &file.path, quoted.relocated);
    // Each entity's lowered code, and then, apart from the code, its local criteria and
    // tests, each on its own line with its id.
    // @lfy def/generation/request.lfy:request#request:request:53047b9ea4a7fce9d7173c9b614c2c73247779aa5d5245a9720c351789d608c1
    for &entity in &unit.entities {
        write_entity(
            out,
            model,
            program,
            unit.lowered,
            entity,
            answered,
            rewordings,
        );
    }
    // The criteria and tests of the file's own entity belong to no declaration of it.
    // @lfy def/generation/request.lfy:request
    let own: Vec<Requirement> = lowered
        .criteria
        .iter()
        .cloned()
        .map(Requirement::Criterion)
        .chain(lowered.tests.iter().cloned().map(Requirement::Test))
        .collect();
    if !own.is_empty() {
        let _ = writeln!(out, "### `{}` (file)\n", file.path);
        write_requirements(out, &own, answered, rewordings);
    }
    // What the outputs answered for and the unit no longer has, last in its section: an id
    // and a text are all that is left of a criterion or a test a unit has dropped.
    // @lfy def/generation/request.lfy:request#request:request:345bea3ab85ad7f91097e0f2d2bb6cb5c995e660d46c32999ad0e500adabfaf6
    if let Some(gone) = quoted.removed.filter(|gone| !gone.is_empty()) {
        out.push_str("### Removed since its outputs were accepted\n\n");
        for requirement in gone {
            // The text of a criterion the outputs answered for is not known where the source
            // they were accepted from is not: only its id was recorded.
            // @lfy def/generation/request.lfy:request#request:request:fad31483b1a56330cc1438cb3020ce5c42a81a5a92c5eb382ce0929be2df4eaa
            let said = if requirement.text.is_empty() {
                "what it said is not known"
            } else {
                &requirement.text
            };
            let _ = writeln!(out, "- {} — {said}", requirement.id);
        }
        out.push('\n');
    }
}

/// The edits other units' runs made inside the unit's regions, in the order they were made:
/// who made each, of which kind, where, in which region, and why, with the region's lines
/// before and after. Left out when there are none.
// @lfy def/generation/request.lfy:request#request:request:5c766480f2aeaf0ebb1ddf1b6a70d47e9bf4cd4b053467207c3a9d2d1eb8b527
fn write_foreign_edits(out: &mut String, target: &Target, foreign_edits: &[ForeignEdit]) {
    if foreign_edits.is_empty() {
        return;
    }
    out.push_str("## Foreign edits\n\n");
    // @lfy def/generation/request.lfy:request#request:request:09e9aef14ec2e21a2597b994510795ad4658cd65c4665c53db6b26e6b2d31698
    out.push_str(
        "These edits were written inside this unit's regions by the runs of the units they name. \
         Keep their code only where it does what this unit's criteria and tests ask.\n\n",
    );
    // @lfy def/generation/request.lfy:request#request:request:29f2379bc0d7e987f6c443d1ffe113cf29d2293392d94b62ebfe545af3055eb0
    if foreign_edits
        .iter()
        .any(|edit| edit.kind == ForeignEditKind::Stub)
    {
        out.push_str(
            "Replace each stub line in this unit's regions by code that does what this unit's \
             criteria and tests ask.\n\n",
        );
    }
    // @lfy def/generation/request.lfy:request#request:request:d64715f05a3c70edd92983d1c91cc1002a00462c6f57ec320e5acb9580bc3cdb
    for (number, edit) in foreign_edits.iter().enumerate() {
        let _ = writeln!(
            out,
            "### {}. A {} by `{}` in `{}`, in the region `{} {}`\n",
            number + 1,
            edit.kind,
            edit.by,
            edit.path,
            target.marker_comment,
            edit.region.spelling()
        );
        let _ = writeln!(out, "Note: {}\n", edit.note);
        let block = |out: &mut String, heading: &str, lines: &[String]| {
            let _ = writeln!(out, "{heading}:\n\n````");
            for line in lines {
                let _ = writeln!(out, "{line}");
            }
            out.push_str("````\n\n");
        };
        block(out, "Before", &edit.before);
        block(out, "After", &edit.after);
    }
}

/// What the existing outputs part says: which they are, that only the difference between the
/// previous lowered source and the current one, the criteria and tests marked new, reworded,
/// or removed, the foreign edits, and a split the target's guidance requires call for a
/// change, that names and structure are otherwise kept, the previous lowered source or that
/// it is not known, and that which criteria are new is not known where no ids were recorded.
/// Left out when nothing exists yet.
// @lfy def/generation/request.lfy:request#request:request:f2c797c5d0ef378166c8076e5ab2728b7f37655f9e7217e2c879e60c4694babf
fn write_existing(out: &mut String, quoted: &Quoted<'_>) {
    if quoted.existing.is_empty() {
        return;
    }
    out.push_str("## Existing outputs\n\n");
    // @lfy def/generation/request.lfy:request#request:request:0ba41c4d98f01ad24856a70c3eb94ada370e2dbc756ad6bb03d09c1593aa935b
    out.push_str("These outputs exist already:\n");
    for path in quoted.existing.keys() {
        let _ = writeln!(out, "- `{path}`");
    }
    // @lfy def/generation/request.lfy:request#request:request:b328124ea1eb5cbf654b1744945d6267f6201a13dbdbf58b20e83731c97b1cf1
    out.push_str(
        "\nChange only what the difference between the previous source and the current source, the \
         criteria and tests marked new, reworded, or removed, the foreign edits, and a split the \
         target's guidance requires, require, and keep the code of an entity relocated into the \
         unit. Keep names and structure, except that a module the target's guidance says to split \
         gains submodule files and has its code moved into them: that split is the one change of \
         structure allowed, and it keeps every name other units use. Keep the markers of \
         unchanged items.\n",
    );
    match quoted.previous {
        // @lfy def/generation/request.lfy:request#request:request:921f0ecc9962f3241a69e55b5e8db934f9a538bdb6ba94b75df67399bf4e7dcf
        Some(text) => {
            let _ = writeln!(
                out,
                "\nThe source the outputs were generated from, lowered as the current source is:\n\n\
                 ```elfie\n{}\n```",
                text.trim_end()
            );
        }
        // A unit whose outputs record no source is reconciled with its source instead.
        // @lfy def/generation/request.lfy:request#request:request:f56fdcbd147d90b70d21fc0a3ca55daee274a966874753ec15588bbcb77df8c2
        None => out.push_str(
            "\nThe source these outputs were generated from is not known. Reconcile the existing \
             outputs with the unit's source.\n",
        ),
    }
    // A unit whose outputs record no ids leaves nothing to say which of its criteria and tests
    // are new, so the existing code is read against every one of them.
    // @lfy def/generation/request.lfy:request#request:request:9dc09496cd7e7f35f205f84cbca429a26f53841256c22511554622cf64d3408e
    if quoted.answered.is_none() {
        out.push_str(
            "\nWhich criteria and tests of the unit are new is not known. Read the existing code \
             against each of them before you write its marker.\n",
        );
    }
    out.push('\n');
}

/// The later units whose code the run may edit, and that no other unit's code is edited.
// @lfy def/generation/request.lfy:request
fn write_reach(out: &mut String, plan: &Plan, reach: &Reach) {
    out.push_str("## The reach of this run\n\n");
    if reach.units.is_empty() {
        // @lfy def/generation/request.lfy:request#request:request:1fe812f0a1e93e2969e57f613ae31f818a433b471cecffbd6f97146481273ba3
        out.push_str("This run edits the outputs of no other unit.\n");
        return;
    }
    // @lfy def/generation/request.lfy:request#request:request:6866b21e4106d263e68cc05d262be33367c3a2efc454a318258613d378049040
    out.push_str(
        "The code of these later units may be edited only as the part \"Stubs and fixes in later \
         units\" says, and no other unit's code is edited:\n\n",
    );
    // @lfy def/generation/request.lfy:request#request:request:7f048ee3afbc169385d5f7d6b40dbfba7314647d732caed0c4723e93ac727dd4
    for &later in &reach.units {
        let unit = &plan.units[later];
        let paths: Vec<String> = unit
            .outputs
            .iter()
            .map(|map| format!("`{}`", map.output))
            .collect();
        let _ = writeln!(out, "- `{}`: {}", unit.stem, paths.join(", "));
    }
}

/// What the compiler is given to read, in the order the request gathered it: a reference
/// or an example either quoted in full or named to be read; a definition as the files its
/// use path resolves to; a tool as the tool of the agent server to call.
// @lfy def/generation/request.lfy:request
fn write_knowledge(out: &mut String, workspace: &Workspace, heading: &str, knowledge: &[Knowledge]) {
    let _ = writeln!(out, "## {heading}\n");
    if knowledge.is_empty() {
        out.push_str("The compiler is given nothing of this kind to read.\n\n");
        return;
    }
    out.push_str("The compiler is given this to read, because it may not already know it:\n\n");
    let mut quoted: Vec<(String, String, String)> = Vec::new();
    for item in knowledge {
        match item.kind {
            // @lfy def/generation/request.lfy:request#request:request:18fa49f6ae0fe059f4d8b65429f7356e822363b0bb901d611f266ca1393c940b
            KnowledgeKind::Tool => {
                let _ = writeln!(
                    out,
                    "- {}: call the tool `{}` of the agent server before relying on what it \
                     answers.",
                    item.topic, item.source
                );
            }
            // @lfy def/generation/request.lfy:request#request:request:179394ea2be57b4a87b41dac2988b30f36b058129a701fc79ff34cc290769606
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
                // @lfy def/generation/request.lfy:request#request:request:d46053a0a71b8d37eb5512c2a9339bc781ffe4a13dfa900ce87d5b17dc1749cd
                let path = under_root(&knowledge_root(workspace, item), &item.source);
                if item.quote == Some(true) {
                    // @lfy def/generation/request.lfy:request#request:request:c08c09e6b0039e6d48af43ad1c7e16111ea4c541b7b9968bfd3cbce3dcbf93cc
                    let text = std::fs::read_to_string(workspace.root.join(&path))
                        .unwrap_or_else(|error| format!("(`{path}` could not be read: {error})"));
                    quoted.push((item.topic.clone(), path, text));
                } else {
                    // @lfy def/generation/request.lfy:request#request:request:4dcb0da9fd03a1602667b395e3dbb573d050278d678ebb597d4e2aba063f5b60
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
// @lfy def/generation/request.lfy:request
// @lfy def/generation/request.lfy:request#request:request:d46053a0a71b8d37eb5512c2a9339bc781ffe4a13dfa900ce87d5b17dc1749cd
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
// @lfy def/generation/request.lfy:request
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
// @lfy def/generation/request.lfy:request#request:request:09b0190ee8969e01b9ff7a561a98c3663f87fb8b97fa83c588d0631e7989bc20
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
    // @lfy def/generation/request.lfy:request#request:request:f700bbb14b11b81fadfe6c18b018250b1c7b9cdadbd0bd8b2a66ce896dfc54c9
    out.push_str(
        "Where the agent server offers `elfie_build`, `elfie_test`, `elfie_lint`, or `elfie_format`, \
         the command of that operation is run through that tool. Run a line by hand only for an \
         operation whose tool is not offered.\n\n",
    );
    // @lfy def/generation/request.lfy:request#request:request:f4b1430a5e2784e149775d2e69bdf4d652782d426521aa999f67d64b6c8a76c5
    out.push_str(
        "The filter of `elfie_test` is given to the test command as `TESTS`, and the scope of \
         `elfie_build`, `elfie_test`, and `elfie_lint` is given to the command as `SCOPE`. Either \
         narrows the run only where the command's line reads it.\n\n",
    );
    // @lfy def/generation/request.lfy:request#request:request:e198f2bcd4f8eb39c9c181816e09b176b6436e859f6c7e7015ba21dcf9e0f4ca
    out.push_str(
        "After the run, the unit is judged with the test command run with stubs allowed, given to \
         it as `ALLOW_STUBS` and through `allowStubs` of `elfie_test`, so that a test failing only \
         by reaching a stub in a later unit's code does not fail the unit. A lint warning fails the \
         unit only in the unit's own code.\n\n",
    );
}

/// The name of an operation, as the standard library spells it.
// @lfy def/generation/request.lfy:request
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
// @lfy def/generation/request.lfy:request
fn write_moves(out: &mut String, workspace: &Workspace, plan: &Plan, index: usize) {
    let model = &workspace.model;
    let spelled = |entities: &[EntityId]| -> String {
        entities
            .iter()
            .map(|&entity| format!("`{}`", entity_name(model, entity)))
            .collect::<Vec<String>>()
            .join(", ")
    };
    // @lfy def/generation/request.lfy:request
    let into: Vec<&Move> = plan
        .moves
        .iter()
        .filter(|moved| moved.into == index)
        .collect();
    if !into.is_empty() {
        // @lfy def/generation/request.lfy:request#request:request:e0d2c5750e61bf64facae6b21f456aaeafa4cfd1be0dc60ff387efee1c092a88
        // @lfy def/generation/request.lfy:request#request:request:ac5358d47d574ae4484013ee8bdc0488e7f6ce892b6e384d1346f27daea31d51
        out.push_str(
            "These entities are compiled in this unit although another file declares them, because \
             the two files would otherwise reach each other. Write them into this unit's outputs \
             with markers naming the file that declares them. Wherever this unit's code needs only \
             them, a `use` of that file is satisfied by them here and never by importing that \
             file.\n",
        );
        // @lfy def/generation/request.lfy:request#request:request:da09691bed9bbd4cea507b74ae9d6919801f1f5948f2c6d2728226a9c8d5f73f
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
    // @lfy def/generation/request.lfy:request
    let outward: Vec<&Move> = plan
        .moves
        .iter()
        .filter(|moved| moved.origin == index)
        .collect();
    if !outward.is_empty() {
        // @lfy def/generation/request.lfy:request#request:request:b30f51561b31df6c26b31d4c397791d154398bc31a9a56efc45eb145fae5800a
        // @lfy def/generation/request.lfy:request#request:request:034c1bdfc4725cdfe7e5eb86af7e47ed43e227a1f7eef528cbb549521620143e
        out.push_str(
            "These entities this file declares are compiled in another unit, because the two files \
             would otherwise reach each other. Import them from that unit's outputs and never write \
             them again, and make each of them available from this unit's outputs under the name \
             and path it would have had, so that code naming it here still finds it.\n",
        );
        // @lfy def/generation/request.lfy:request#request:request:0f1d87f11591131ce4052dfc72bc179de20f377a5b220fb4cb1cd3f9237e0d4e
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

/// What was relocated into and out of the unit whose file is `path`, in its section: which
/// entities, from or into which file, and what becomes of their code.
// @lfy def/generation/request.lfy:request
fn write_relocations(out: &mut String, path: &str, relocated: &[Relocation]) {
    let into: Vec<&Relocation> = relocated
        .iter()
        .filter(|relocation| relocation.into == path)
        .collect();
    if !into.is_empty() {
        // @lfy def/generation/request.lfy:request#request:request:8ef7cabaa1ffd44e92776ee0d9dcb63c0fc0f45c81a6d02e6e4a581fa179daad
        out.push_str(
            "These entities were relocated into this unit, since this unit's file declares them now. \
             Their code is already in this unit's outputs, under markers that now name this unit's \
             file. Keep it as it is, and change it only where what this unit asks differs from what \
             it does.\n",
        );
        for relocation in into {
            let _ = writeln!(out, "- `{}` from `{}`.", relocation.entity, relocation.origin);
        }
        out.push('\n');
    }
    let outward: Vec<&Relocation> = relocated
        .iter()
        .filter(|relocation| relocation.origin == path)
        .collect();
    if !outward.is_empty() {
        // @lfy def/generation/request.lfy:request#request:request:b061d31417b089c8b88fe3fcd63a32d6608ce44a82cd1e7b845e3bd2daa5e079
        out.push_str(
            "These entities were relocated out of this unit, since another file declares them now. \
             Their code is the code of the unit of that file now: neither write it again nor remove \
             it.\n",
        );
        for relocation in outward {
            let _ = writeln!(out, "- `{}` into `{}`.", relocation.entity, relocation.into);
        }
        out.push('\n');
    }
}

/// One entity of a unit: its name, its kind, its definition, its type, its lowered code,
/// and then, apart from the code, its local criteria and tests, each on its own line with
/// its id.
// @lfy def/generation/request.lfy:request
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
        "### `{}` ({})\n",
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
    // @lfy def/generation/request.lfy:request
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

/// Every criterion and test of a list, criteria then tests, each on one line reading its id,
/// a space, an em dash, a space, and its text: what a request and a review quote apart from
/// the code.
///
/// Where `answered` holds the ids the existing outputs were generated against, a line whose
/// id is none of them is marked new, so that a compiler reading existing code knows which of
/// them that code was never held to. A line whose id rewords one of them is marked reworded
/// instead, and the id and the text it replaces follow it on a line of their own.
// @lfy def/generation/request.lfy:request#request:request:258d7f943a5b84ea809c220868bbf11249a423cd3891bd01fced30a1d9b5bfc1
// @lfy def/generation/request.lfy:request#request:request:aa208260d2e6de0481fdd00165686f8ac50d4c5662ac4e1445cbaecdfb556b09
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
    // @lfy def/generation/request.lfy:request#request:request:0bc1580cb79537e04f3b1d9cc19ee8db2fb9277b47ffe12994bb0d31371cbee8
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
            "- {} — {}{}",
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
            "- {} — {}{}",
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
// @lfy def/generation/request.lfy:request
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
// @lfy def/generation/request.lfy:request
// @lfy def/generation/review.lfy:review
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
/// The file is the one at `path` in [`Previous::program`], lowered once when it was
/// recovered, so nothing is lowered or bound here; its text is the lowered text a request
/// carries for the file now and the two can be read side by side, and the ids come out
/// spelled as the unit's own are. `None` when that program holds no file at that path.
// @lfy def/generation/request.lfy:request#request:request:79b8e2cfc5abf587f5ee588823cd91a2bc93bca07e5e13d1723444029d15b27f
// @lfy def/generation/request.lfy:request#request:request:a5c47f2c55a69433503b4e3695c342527d245e9af1cf95c78e340d2dafe6c4d9
fn file_as_it_was(previous: &Previous, path: &str) -> Option<(String, Vec<RemovedRequirement>)> {
    let before = &previous.program;
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
