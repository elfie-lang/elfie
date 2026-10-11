// @lfy def/mcp/main.lfy:units
//! The tools that read what the compiler would do: the plan of its units and batches, the
//! request for one batch, whether a unit's outputs would be accepted, and what differs for
//! a unit since its outputs were.

use super::*;

/// One problem of a plan as a line: as `problems` spells a diagnostic of stage generation
/// and severity error at its node.
// @lfy def/mcp/main.lfy:units#units:units:e3b6f3a30f32b79ed993570429ffa615792d56cc2481b2b41aa6f859567ec450
fn problem_line(model: &Model, problem: &Problem) -> String {
    let (file, line, column) = match model.first_token(problem.node) {
        Some(token) => (token.file.to_string(), token.line, token.column),
        None => (model.sources[problem.node.file].path.clone(), 0, 0),
    };
    diagnostic_line(&file, line, column, &problem.stage.to_string(), "error", &problem.message)
}

/// One move of a plan as a line: the word move, the target, the stem it came from, the word
/// into, the stem that compiles it, the identifiers moved in brackets, the word for, and
/// what caused it.
// @lfy def/mcp/main.lfy:units#units:units:04942eee0b664f2b18f16b21c1e245d6ec558e513af796471b999cea68d090c6
pub(crate) fn move_line(workspace: &Workspace, plan: &Plan, moved: &Move) -> String {
    let model = &workspace.model;
    let origin = &plan.units[moved.origin];
    let entities: Vec<String> = moved.entities.iter().map(|&entity| identifier_of(model, entity)).collect();
    format!(
        "move {} {} into {} [{}] for {}",
        workspace.targets[origin.target].identifier,
        origin.stem,
        plan.units[moved.into].stem,
        entities.join(", "),
        moved.cause
    )
}

/// One unit as a line: the target, the stem, the reason or up to date, and the stems of
/// its dependencies.
// @lfy def/mcp/main.lfy:units#units:units:c7fcb0ca869f9f4555904c7d48858c0314cebdf85dc76b32442da4fcb97e9021
fn unit_line(workspace: &Workspace, plan: &Plan, index: usize) -> String {
    let unit = &plan.units[index];
    let reason = unit.reason.map_or_else(|| "up to date".to_string(), |reason| reason.as_str().to_string());
    let dependencies: Vec<&str> = unit.dependencies.iter().map(|&dependency| plan.units[dependency].stem.as_str()).collect();
    format!("{} {} {reason} [{}]", workspace.targets[unit.target].identifier, unit.stem, dependencies.join(", "))
}

/// One batch as a line: its identifier and the stems it holds.
// @lfy def/mcp/main.lfy:units#units:units:92120a7e31d7970c4faf3dd986b56767f95eeeb9e0470fd818c092f986d3e0d3
fn batch_line(plan: &Plan, batch: &Batch) -> String {
    let stems: Vec<&str> = batch.units.iter().map(|&unit| plan.units[unit].stem.as_str()).collect();
    format!("batch {} [{}]", batch.identifier, stems.join(", "))
}

/// The target of the unit a problem's node is in, when the node is in a unit's file; a
/// problem outside every unit's file is of no one target.
// Decision: a plan problem names a node, not a target, so the target narrows the problems
// by the file the node is in, which is the file of the unit the problem is about.
// @lfy def/mcp/main.lfy:units#units:units:05a79e1b79428d09f369fcfe7a098f9648d1dbfb976d11d8d457dd8945737272
fn problem_targets<'w>(workspace: &'w Workspace, plan: &Plan, problem: &Problem) -> Vec<&'w str> {
    let path = &workspace.model.sources[problem.node.file].path;
    plan.units
        .iter()
        .filter(|unit| &workspace.files[unit.file].path == path)
        .map(|unit| workspace.targets[unit.target].identifier.as_str())
        .collect()
}

/// The units the compiler would produce, in order, why each needs generating, and how
/// they are batched: the problems, then the moves, then the units, then the batches.
// @lfy def/mcp/main.lfy:units#units:units:3dfafdff61f6c1061f2fe64e135a6885c4554344d56b669fcd2f9dceddc9bb98
pub fn units(session: &Session, target: Option<&str>) -> Result<String, String> {
    check_target(&session.workspace, target)?;
    let planned = plan_of(session);
    let (workspace, plan) = (planned.workspace(), &planned.plan);
    let model = &workspace.model;
    // The problems come first, each as one line. A problem of a file no unit of the named
    // target holds is left out with that target's units.
    // @lfy def/mcp/main.lfy:units#units:units:e3b6f3a30f32b79ed993570429ffa615792d56cc2481b2b41aa6f859567ec450
    let mut lines: Vec<String> = plan
        .problems
        .iter()
        .filter(|problem| target.is_none_or(|target| problem_targets(workspace, plan, problem).contains(&target)))
        .map(|problem| problem_line(model, problem))
        .collect();
    // Then the moves, each as one line.
    // @lfy def/mcp/main.lfy:units#units:units:04942eee0b664f2b18f16b21c1e245d6ec558e513af796471b999cea68d090c6
    lines.extend(
        plan.moves
            .iter()
            .filter(|moved| {
                target.is_none_or(|target| workspace.targets[plan.units[moved.origin].target].identifier == target)
            })
            .map(|moved| move_line(workspace, plan, moved)),
    );
    // Then the units, each as one line.
    // @lfy def/mcp/main.lfy:units#units:units:05a79e1b79428d09f369fcfe7a098f9648d1dbfb976d11d8d457dd8945737272
    let units: Vec<String> = (0..plan.units.len())
        .filter(|&index| target.is_none_or(|target| workspace.targets[plan.units[index].target].identifier == target))
        .map(|index| unit_line(workspace, plan, index))
        .collect();
    if units.is_empty() {
        return Ok(match target {
            Some(target) => format!("no units for the target {target}"),
            None => "no units".to_string(),
        });
    }
    lines.extend(units);
    // Then one line per batch. Decision: a batch holds units of one target, so the
    // target narrows the batches the same way it narrows the units; a blank line and the
    // word batch keep the two lists apart, since a batch line has the shape of a unit's.
    // @lfy def/mcp/main.lfy:units#units:units:92120a7e31d7970c4faf3dd986b56767f95eeeb9e0470fd818c092f986d3e0d3
    let batches: Vec<String> = plan
        .batches
        .iter()
        .filter(|batch| target.is_none_or(|target| batch_target(workspace, plan, batch) == target))
        .map(|batch| batch_line(plan, batch))
        .collect();
    if !batches.is_empty() {
        lines.push(String::new());
        lines.extend(batches);
    }
    Ok(lines.join("\n"))
}

/// The full request for one batch: what to produce, where, every criterion to satisfy,
/// and how to report.
// @lfy def/mcp/main.lfy:request#request:request:92537a4ab2aac8027576852406e446fcb12c7b5b87c96095a386c6d7dac101ed
pub fn request(session: &Session, batch: &str, target: Option<&str>) -> Result<String, String> {
    let planned = plan_of(session);
    let plan = &planned.plan;
    // The plan has problems, so there is no request: an entity that reaches one not built
    // for its target, or a cycle no move resolved, leaves every unit of the plan doubtful.
    // This is read before the batch is resolved, because a plan with problems may hold no
    // batch for the name at all and the criterion names no exception.
    // @lfy def/mcp/main.lfy:request#request:request:2bea1608ac7ace6999af5d0abfb20a8166b6e5149871e97cdc2660b9709891e0
    if !plan.problems.is_empty() {
        let model = &planned.workspace().model;
        let mut text = "the plan has problems, so there is no request:".to_string();
        for problem in &plan.problems {
            text.push_str(&format!("\n  {}", problem_line(model, problem)));
        }
        return Err(text);
    }
    let index = resolve_batch(planned.workspace(), plan, batch, target)?; // @lfy def/mcp/main.lfy:request#request:request:ca0a25d6a947aa41d3cbf9399a371f681224987d6109d85e16a0b7f8a85db14c
    // A hosted session answers for one run of one agent, so it is given the request of its
    // own batch and of no other.
    // @lfy def/mcp/main.lfy:request#request:request:f7ef0e526d107bc01f393317930dc6691e83904a9ec731aef3e304884867cfe0
    own_batch(session, &plan.batches[index].identifier)?;
    // A compiler is given the request it was run with, so what it is asked to produce is what
    // the compile asked it, never a request made again from the definitions as they are now;
    // every other session has the request made for the batch.
    // @lfy def/mcp/main.lfy:request#request:request:af5eea7b402d3ed59ce0541ca71250ff92ea2e142a768f4b98af9e9bc4f348c9
    if let Some(binding) = &session.binding
        && binding.role == Role::Compiler
        && let Some(given) = &binding.request
    {
        return Ok(given.instructions.clone());
    }
    Ok(request_for(&planned.program, plan, &plan.batches[index]).instructions) // @lfy def/mcp/main.lfy:request#request:request:92537a4ab2aac8027576852406e446fcb12c7b5b87c96095a386c6d7dac101ed
}

/// Whether the outputs on disk satisfy the request for one unit, and what is wrong when
/// they do not. Nothing is written; the CLI records source maps.
// @lfy def/mcp/main.lfy:check#check:check:860750a195c0ff4eb9ed95b87939e831cae5a22fecd5d7efe17858c8f124ff9d
pub fn check(session: &Session, unit: &str, target: Option<&str>) -> Result<String, String> {
    let planned = plan_of(session);
    let (workspace, plan) = (planned.workspace(), &planned.plan);
    let index = resolve_unit(workspace, plan, unit, target)?;
    // A hosted session checks a unit of its own batch and nothing else, so nothing is
    // checked when it is asked about another.
    // @lfy def/mcp/main.lfy:check#check:check:9c073d6b09e272ea0d5a712167c00378477b35e103dad16445f0d53b88d7bc30
    own_unit(session, &plan.units[index].stem)?;
    let request = request_of_unit(&planned.program, plan, index);
    // @lfy def/mcp/main.lfy:check#check:check:860750a195c0ff4eb9ed95b87939e831cae5a22fecd5d7efe17858c8f124ff9d
    let (outputs, elsewhere) = outputs_of(workspace, plan, index);
    // What the last acceptance recorded for the target's other units, so that an output
    // another unit is to write is seen as that unit's rather than as this one's.
    // @lfy def/mcp/main.lfy:check#check:check:860750a195c0ff4eb9ed95b87939e831cae5a22fecd5d7efe17858c8f124ff9d
    let source = workspace.files[plan.units[index].file].path.clone();
    let identifier = workspace.targets[plan.units[index].target].identifier.clone();
    let recorded: Vec<SourceMap> = generation::source_maps_of(workspace, Some(&identifier))
        .into_iter()
        .filter(|map| map.source != source)
        .collect();
    let verdict = generation::accept(&planned.program, plan, &request, index, &outputs, &recorded, &elsewhere);
    if verdict.accepted {
        // @lfy def/mcp/main.lfy:check#check:check:7d045cb553e0d306e634e78ed9a299647790d4beae5075ece0962d22ff2d4ef8
        let mut text = format!(
            "accepted {} for {}: {} output{}",
            plan.units[index].stem,
            workspace.targets[plan.units[index].target].identifier,
            outputs.len(),
            if outputs.len() == 1 { "" } else { "s" }
        );
        for output in &outputs {
            text.push_str(&format!("\n  {}", output.path));
        }
        text.push_str("\nnothing was written; the CLI records the source maps");
        Ok(text)
    } else {
        // @lfy def/mcp/main.lfy:check#check:check:2a16ea176547468dbe87eb74de7c195dc58e16c26122fc69de5f2ff15dd469f1
        let mut text = format!("rejected {}:", plan.units[index].stem);
        for problem in &verdict.problems {
            text.push_str(&format!("\n  {problem}"));
        }
        Err(text)
    }
}

/// The unit with a stem, in the target given or, when none is given, of the first target in
/// `Workspace::targets` order that has a unit with that stem.
// @lfy def/mcp/main.lfy:changes#changes:changes:21683ec505359b2effe3c7d8bc5d8fbcc63e581f01406ec3948d214f74916b65
pub(crate) fn unit_of_stem(workspace: &Workspace, plan: &Plan, stem: &str, target: Option<&str>) -> Result<usize, String> {
    check_target(workspace, target)?; // @lfy def/mcp/main.lfy:changes#changes:changes:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
    for index in 0..workspace.targets.len() {
        if target.is_some_and(|target| workspace.targets[index].identifier != target) {
            continue;
        }
        if let Some(found) = (0..plan.units.len())
            .find(|&unit| plan.units[unit].stem == stem && plan.units[unit].target == index)
        {
            return Ok(found);
        }
    }
    // @lfy def/mcp/main.lfy:changes#changes:changes:bbfd13869e5d8aa4ccdda753d25b4797e5140bfb3b5f287cf8f83310a3b51e7b
    Err(match target {
        Some(target) => format!("no unit of the target {target} has the stem {stem}; elfie_units lists them"),
        None => format!("no unit has the stem {stem}; elfie_units lists them"),
    })
}

/// What was recovered of the source one unit's outputs were accepted from.
// @lfy def/mcp/main.lfy:changes#changes:changes:56177984bed3a3685a94d23b96a8f9cbc1f37f2aae411cc9a9e89a6839bcab09
pub(crate) enum Recovered {
    /// The source its outputs were accepted from.
    Accepted(String),
    /// Only the latest revision of its file whose lowered code is the one the outputs were
    /// accepted from, which says nothing about which criteria and tests they answered for.
    // @lfy def/mcp/main.lfy:changes#changes:changes:ae710bf66643404415b338248d178927e1ce5abd1905f360bf7bfb0ec58ef93a
    HashMatched(String),
    /// Neither.
    // @lfy def/mcp/main.lfy:changes#changes:changes:0ee7a53de30f39d09aac719e1777c20ac77f3790000eab2781f26813b23a5bc8
    Nothing,
}

impl Recovered {
    /// The previous text to pass to `generation::changes`: the revision as git holds it,
    /// never lowered, and nothing when neither source could be recovered.
    // @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
    pub(crate) fn text(&self) -> Option<&str> {
        match self {
            Recovered::Accepted(text) | Recovered::HashMatched(text) => Some(text),
            // @lfy def/mcp/main.lfy:changes#changes:changes:0ee7a53de30f39d09aac719e1777c20ac77f3790000eab2781f26813b23a5bc8
            Recovered::Nothing => None,
        }
    }
}

/// What one git command wrote to standard output, run in the workspace root; `None` when it
/// could not be run or did not succeed, as it does not in a root that is no repository.
// @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let run = Command::new("git")
        .args(args)
        .current_dir(root)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !run.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&run.stdout).into_owned())
}

/// One revision of a unit's file, planned as the workspace would plan it with that revision
/// in place: the program lowered from it, the plan, and the unit of the same stem for the
/// same target.
///
/// The revision is bound through the loader and lowered the way `generation::changes` binds
/// a previous text, and planned so that what the file declared then is gathered into a unit
/// the same way, which is what a source map holds the hash and the answered ids of.
// @lfy def/mcp/main.lfy:changes#changes:changes:56177984bed3a3685a94d23b96a8f9cbc1f37f2aae411cc9a9e89a6839bcab09
fn planned_before(workspace: &Workspace, unit: &Unit, text: &str) -> Option<(Program, Plan, usize)> {
    let path = workspace.files[unit.file].path.clone();
    let identifier = workspace.targets[unit.target].identifier.clone();
    // @lfy def/mcp/main.lfy:changes#changes:changes:56177984bed3a3685a94d23b96a8f9cbc1f37f2aae411cc9a9e89a6839bcab09
    let (program, plan) = generation::plan(workspace::change(workspace, &path, Some(text)), &[], &[], &[]);
    let index = (0..plan.units.len()).find(|&index| {
        plan.units[index].stem == unit.stem
            && program.workspace.targets[plan.units[index].target].identifier == identifier
    })?;
    Some((program, plan, index))
}

/// What can be recovered of the source at the unit's last accepted generation, as the
/// command line recovers it: through git when the root is a repository and the unit's source
/// is in it, reading the revisions of the file from the most recent back.
///
/// What is given back is a revision as git holds it, never lowered, because that is what
/// `generation::changes` takes; a revision is *recognized* by lowering, since the map holds
/// the hash of the lowered text rather than of the file's bytes, so a revision differing only
/// in a comment lowers to the same code. Lowered code is not enough on its own: a criterion
/// reworded leaves it untouched, so the revision the outputs were accepted from is the one
/// that also has the ids the map records as answered, and where no revision has them the
/// latest one with the same lowered code is all there is.
// @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
// @lfy def/mcp/main.lfy:changes#changes:changes:56177984bed3a3685a94d23b96a8f9cbc1f37f2aae411cc9a9e89a6839bcab09
pub(crate) fn previous_source(workspace: &Workspace, unit: &Unit) -> Recovered {
    let Some(map) = unit.outputs.first() else { return Recovered::Nothing };
    let file = workspace.files[unit.file].path.clone();
    // @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
    // Every revision of the file, from the most recent back, with no count to cut the search
    // short: the revision the outputs were accepted from is recognized however old it is.
    // @lfy def/mcp/main.lfy:changes#changes:changes:56177984bed3a3685a94d23b96a8f9cbc1f37f2aae411cc9a9e89a6839bcab09
    let Some(log) = git(&workspace.root, &["log", "--format=%H", "--", &file]) else {
        return Recovered::Nothing;
    };
    let mut matched: Option<String> = None;
    for revision in log.lines() {
        let Some(text) = git(&workspace.root, &["show", &format!("{revision}:{file}")]) else { continue };
        let Some((program, plan, index)) = planned_before(workspace, unit, &text) else { continue };
        let before = &plan.units[index];
        if generation::source_hash(&program.files[before.lowered].text) != map.hash {
            continue;
        }
        // The most recent revision whose lowered code is the one the outputs were accepted
        // from; it stands in only if none of them is the source they were accepted from.
        // @lfy def/mcp/main.lfy:changes#changes:changes:ae710bf66643404415b338248d178927e1ce5abd1905f360bf7bfb0ec58ef93a
        if matched.is_none() {
            matched = Some(text.clone());
        }
        // A source map records the ids of the unit's local criteria and tests as they were
        // when its outputs were accepted, as their hash: sorted and joined by line breaks.
        // The revision they were accepted from is the one whose ids hash to it. A map
        // written before that was recorded holds no hash at all, and then no revision is
        // recognized as that source.
        // @lfy def/mcp/main.lfy:changes#changes:changes:56177984bed3a3685a94d23b96a8f9cbc1f37f2aae411cc9a9e89a6839bcab09
        let mut ids: Vec<String> = before
            .requirements(&program)
            .iter()
            .map(|requirement| requirement.id().to_string())
            .collect();
        ids.sort();
        if !map.requirements.is_empty() && generation::source_hash(&ids.join("\n")) == map.requirements {
            return Recovered::Accepted(text);
        }
    }
    // @lfy def/mcp/main.lfy:changes#changes:changes:ae710bf66643404415b338248d178927e1ce5abd1905f360bf7bfb0ec58ef93a
    match matched {
        Some(text) => Recovered::HashMatched(text),
        None => Recovered::Nothing,
    }
}

/// What differs for each entity of a unit since its outputs were last accepted.
// @lfy def/mcp/main.lfy:changes#changes:changes:21683ec505359b2effe3c7d8bc5d8fbcc63e581f01406ec3948d214f74916b65
pub fn changes(session: &Session, unit: &str, target: Option<&str>) -> Result<String, String> {
    let planned = plan_of(session);
    let workspace = planned.workspace();
    let index = unit_of_stem(workspace, &planned.plan, unit, target)?;
    // A hosted session answers for a unit of its own batch and no other.
    // @lfy def/mcp/main.lfy:changes#changes:changes:e97791dbc253b4037be4b5a6a54aa430027a02f99710070c689449608a3affe1
    own_unit(session, &planned.plan.units[index].stem)?;
    let of_stem = &planned.plan.units[index];
    if of_stem.outputs.is_empty() {
        return Ok(format!("no previous output for {unit}; nothing has been accepted for it yet")); // @lfy def/mcp/main.lfy:changes#changes:changes:413343511b79d06a29fa38852b4bf26d600fdb0e411965a13d3190ca9375546e
    }
    let recovered = previous_source(workspace, of_stem); // @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
    let mut text = String::new();
    match &recovered {
        Recovered::Accepted(_) => {}
        // @lfy def/mcp/main.lfy:changes#changes:changes:ae710bf66643404415b338248d178927e1ce5abd1905f360bf7bfb0ec58ef93a
        Recovered::HashMatched(_) => text.push_str(
            "the source its outputs were accepted from is not known; the changes below are against \
             the latest revision of its file with the same lowered code, so the criteria and tests \
             they report as changed may not be those that changed since its outputs were accepted\n",
        ),
        // @lfy def/mcp/main.lfy:changes#changes:changes:0ee7a53de30f39d09aac719e1777c20ac77f3790000eab2781f26813b23a5bc8
        Recovered::Nothing => text.push_str(
            "neither the source its outputs were accepted from nor a revision of its file with the \
             same lowered code can be recovered; every entity is listed as added\n",
        ),
    }
    let found = generation::changes(&planned.program, of_stem, recovered.text()); // @lfy def/mcp/main.lfy:changes#changes:changes:21683ec505359b2effe3c7d8bc5d8fbcc63e581f01406ec3948d214f74916b65
    if found.is_empty() {
        // @lfy def/mcp/main.lfy:changes#changes:changes:35a395b2402b76e8ae9d6c636a82478d28714ed6e6459fe96e70847ff73470a4
        text.push_str(&format!("no changes in {unit} since its outputs were accepted"));
        return Ok(text);
    }
    // @lfy def/mcp/main.lfy:changes#changes:changes:21683ec505359b2effe3c7d8bc5d8fbcc63e581f01406ec3948d214f74916b65
    let lines: Vec<String> = found
        .iter()
        .map(|change| format!("{} {} {}", change.entity, change.kind.as_str(), change.detail))
        .collect();
    text.push_str(&lines.join("\n"));
    Ok(text)
}
