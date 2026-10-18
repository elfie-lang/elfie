// @lfy def/mcp/main.lfy:units
//! The tools that read what the compiler would do: the plan of its units, the request for
//! one unit, whether a unit's outputs would be accepted, and what differs for
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

/// The target of the unit a problem's node is in, when the node is in a unit's file; a
/// problem outside every unit's file is of no one target.
// Decision: a plan problem names a node, not a target, so the target narrows the problems
// by the file the node is in, which is the file of the unit the problem is about.
// @lfy def/mcp/main.lfy:units#units:units:9ef7e980a808516fa1e3bd6a7a7746a57128c4ef775e20c6b6ebf41db491c432
fn problem_targets<'w>(workspace: &'w Workspace, plan: &Plan, problem: &Problem) -> Vec<&'w str> {
    let path = &workspace.model.sources[problem.node.file].path;
    plan.units
        .iter()
        .filter(|unit| &workspace.files[unit.file].path == path)
        .map(|unit| workspace.targets[unit.target].identifier.as_str())
        .collect()
}

/// The units the compiler would produce, in the order they are compiled one at a time, and
/// why each needs generating: the problems, then the moves, then the units.
// @lfy def/mcp/main.lfy:units#units:units:920638da45d13abf0859774d9b78a3a3087c59fed19e9fe90e97b355194204b6
pub fn units(session: &Session, target: Option<&str>) -> Result<String, String> {
    check_target(&session.workspace, target)?;
    // A hosted session shows the plan its compile made; any other plans the workspace.
    // @lfy def/mcp/main.lfy:units#units:units:be4f6c886ef6e1db9b9b5072b338544e713652abd61a55caba16263e8386d4c4
    let planned = shown_plan(session);
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
    // @lfy def/mcp/main.lfy:units#units:units:9ef7e980a808516fa1e3bd6a7a7746a57128c4ef775e20c6b6ebf41db491c432
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
    Ok(lines.join("\n"))
}

/// The full request for one unit: what to produce, where, every criterion to satisfy, the
/// edits other units made to its outputs, the later units it may edit, and how to report.
// @lfy def/mcp/main.lfy:request#request:request:baffffb532c00461b7b09f3370bae9624180e5c983807908f6791b256ad14b55
pub fn request(session: &Session, unit: &str, target: Option<&str>) -> Result<String, String> {
    // The plan is the one elfie_units shows.
    let planned = shown_plan(session);
    let plan = &planned.plan;
    // The plan has problems, so there is no request: an entity that reaches one not built
    // for its target, or a cycle no move resolved, leaves every unit of the plan doubtful.
    // This is read before the unit is resolved, because the criterion names no exception.
    // @lfy def/mcp/main.lfy:request#request:request:2bea1608ac7ace6999af5d0abfb20a8166b6e5149871e97cdc2660b9709891e0
    if !plan.problems.is_empty() {
        let model = &planned.workspace().model;
        let mut text = "the plan has problems, so there is no request:".to_string();
        for problem in &plan.problems {
            text.push_str(&format!("\n  {}", problem_line(model, problem)));
        }
        return Err(text);
    }
    let index = resolve_unit(planned.workspace(), plan, unit, target)?; // @lfy def/mcp/main.lfy:request#request:request:35e5ad0177134a3f3f1deca8a9b000be17f11403bc5bb2ae319face434d2913f
    // A hosted session answers for one run of one agent, so it is given the request of its
    // own unit and of no other.
    // @lfy def/mcp/main.lfy:request#request:request:48f89e4f5dbd19355149dfdab15f569200b5dc13cfbdb7524dd3cfabd7052097
    own_unit(session, plan, index)?;
    // A compiler is given the request it was run with, so what it is asked to produce is what
    // the compile asked it, never a request made again from the definitions as they are now;
    // every other session has the request made for the unit.
    // @lfy def/mcp/main.lfy:request#request:request:2f93fec1bd3f115335d968e64f7a1aefff036a5ba850d09ae602a280cc9d52ad
    if let Some(binding) = &session.binding
        && binding.role == Role::Compiler
        && let Some(given) = &binding.request
    {
        return Ok(given.instructions.clone());
    }
    Ok(request_for(&planned.program, plan, index).instructions) // @lfy def/mcp/main.lfy:request#request:request:baffffb532c00461b7b09f3370bae9624180e5c983807908f6791b256ad14b55
}

/// One foreign edit an accepted verdict holds, as a line: its kind, the word in, its owner,
/// its path, a colon, the line its region begins on, and how many lines it changes.
fn edit_line(edit: &ForeignEdit) -> String {
    format!(
        "{} in {} {}:{} {} lines",
        edit.kind.as_str(),
        edit.owner,
        edit.path,
        edit.region.output_line,
        edit.changed_lines()
    )
}

/// Whether the outputs on disk satisfy the request for one unit, and what is wrong when
/// they do not. Nothing is written; the CLI records source maps.
// @lfy def/mcp/main.lfy:check#check:check:d517596f37da6f2ebd9a8bac21cc5e470ac25921376da26b3cafa005e22b51ee
pub fn check(session: &Session, unit: &str, target: Option<&str>) -> Result<String, String> {
    let planned = shown_plan(session);
    let (workspace, plan) = (planned.workspace(), &planned.plan);
    let index = resolve_unit(workspace, plan, unit, target)?;
    // A hosted session checks its own unit and nothing else, so nothing is checked when it
    // is asked about another.
    // @lfy def/mcp/main.lfy:check#check:check:de01f24fe16815616e98694787fdc90451dd8e2c6a0803dcf9936fb3f92eb1bf
    own_unit(session, plan, index)?;
    let compiler = session.binding.as_ref().filter(|binding| binding.role == Role::Compiler);
    // @lfy def/mcp/main.lfy:check#check:check:45672f7351ea405dd7ffc891b1f97edf8bc53b0c6318964ac46234940a594410
    let given = compiler.and_then(|binding| binding.request.as_ref()).filter(|_| is_bound_unit(session, plan, index));
    let made;
    let request = match given {
        Some(given) => given,
        None => {
            // @lfy def/mcp/main.lfy:check#check:check:99ef27a8e37ec8ae852a1fbed7fb641118f4d212b8dc0298bd44088933fa5488
            made = request_for(&planned.program, plan, index);
            &made
        }
    };
    // @lfy def/mcp/main.lfy:check#check:check:3475f72c3ad61b4c4f2156a3726b40e635a69204d88169984cdadd42fdcc2cf1
    // @lfy def/mcp/main.lfy:check#check:check:78908963fd2ee7bcbad8fac4938f354a4ad423f802bdec1e1f80b478611f8f74
    let before: Option<&[Output]> = compiler.and_then(|binding| binding.before.as_deref());
    let touches = session.touched();
    // @lfy def/mcp/main.lfy:check#check:check:d517596f37da6f2ebd9a8bac21cc5e470ac25921376da26b3cafa005e22b51ee
    let (outputs, elsewhere) = outputs_of(workspace, plan, index);
    // What the last acceptance recorded for the target's other units, so that an output
    // another unit is to write is seen as that unit's rather than as this one's.
    // @lfy def/mcp/main.lfy:check#check:check:d517596f37da6f2ebd9a8bac21cc5e470ac25921376da26b3cafa005e22b51ee
    let source = workspace.files[plan.units[index].file].path.clone();
    let identifier = workspace.targets[plan.units[index].target].identifier.clone();
    let recorded: Vec<SourceMap> = generation::source_maps_of(workspace, Some(&identifier))
        .into_iter()
        .filter(|map| map.source != source)
        .collect();
    let verdict = generation::accept(
        &planned.program,
        plan,
        request,
        index,
        &outputs,
        &recorded,
        &elsewhere,
        before,
        &touches,
    );
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
        // @lfy def/mcp/main.lfy:check#check:check:3fbbf8a1ad1e7e731818a436899a0f61e7d0c7505c0f22229556e2a35f6568c1
        for edit in &verdict.foreign_edits {
            text.push_str(&format!("\n{}", edit_line(edit)));
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
    let (program, plan) = generation::plan(workspace::change(workspace, &path, Some(text)), &[], &[], &[], &[]);
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
    // A hosted session answers for its own unit and no other.
    // @lfy def/mcp/main.lfy:changes#changes:changes:47939b7e26a096d51016320d59ba7e3a38ffd7f43690c663bdeb522b5f87dcf3
    own_unit(session, &planned.plan, index)?;
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
