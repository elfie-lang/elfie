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

/// The units the compiler would produce, in the order they are compiled, and why each
/// needs generating: the problems, then the moves, then the units.
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
// @lfy def/mcp/main.lfy:request#request:request:458d02f36175e2302ccdf1f4a0a5e4f6427d6ad3caafb42b339a713769527d7f
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
    Ok(request_for(&planned.program, plan, index).instructions) // @lfy def/mcp/main.lfy:request#request:request:458d02f36175e2302ccdf1f4a0a5e4f6427d6ad3caafb42b339a713769527d7f
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
// @lfy def/mcp/main.lfy:check#check:check:c6866961ccb185b658d527b660124a894f5a6e4c4c8a6b6b79f8d40199fd33a7
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
    // @lfy def/mcp/main.lfy:check#check:check:f62b2808006bb1ede584a2e554e66f850b587343f1593b99f7b47aeb05e5de6f
    // @lfy def/mcp/main.lfy:check#check:check:c38c9a07a3699d11511265746c3a3d56f97ca029e28d14f4f2fbc5c31e955939
    let before: Option<&[Output]> = compiler.and_then(|binding| binding.before.as_deref());
    let touches = session.touched();
    // @lfy def/mcp/main.lfy:check#check:check:c6866961ccb185b658d527b660124a894f5a6e4c4c8a6b6b79f8d40199fd33a7
    let (outputs, elsewhere) = outputs_of(workspace, plan, index);
    // What the last acceptance recorded for the target's other units, so that an output
    // another unit is to write is seen as that unit's rather than as this one's.
    // @lfy def/mcp/main.lfy:check#check:check:c6866961ccb185b658d527b660124a894f5a6e4c4c8a6b6b79f8d40199fd33a7
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
        // @lfy def/mcp/main.lfy:check#check:check:f3f55cf806901e6c7c5de76844080355b2901cae32c8244bf1c4109365eb069d
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
        // @lfy def/mcp/main.lfy:check#check:check:118b7ce146ef04014cced08a2301ca2de11ee4af2a1bab5bb2eb970a0fa0b9c7
        for edit in &verdict.foreign_edits {
            text.push_str(&format!("\n{}", edit_line(edit)));
        }
        text.push_str("\nnothing was written; the CLI records the source maps");
        Ok(text)
    } else {
        // @lfy def/mcp/main.lfy:check#check:check:aa65fce675d53a43132ff27e80f270948be090228ac8b39998f2eb2375b5519a
        let mut text = format!("rejected {}:", plan.units[index].stem);
        for problem in &verdict.problems {
            text.push_str(&format!("\n  {problem}"));
        }
        Err(text)
    }
}

/// The unit with a stem, in the target given or, when none is given, of the first target in
/// `Workspace::targets` order that has a unit with that stem.
// @lfy def/mcp/main.lfy:changes#changes:changes:76805f808eadb6afc31288f850876553118dd0066c28d603b5716c112369b09c
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

/// What differs for each entity of a unit since its outputs were last accepted.
// @lfy def/mcp/main.lfy:changes#changes:changes:76805f808eadb6afc31288f850876553118dd0066c28d603b5716c112369b09c
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
    // The source the outputs were accepted from, recovered once from the program the plan
    // was made from and lowered once, as `generation::changes` takes it.
    // @lfy def/mcp/main.lfy:changes#changes:changes:53f95c52d3400ab227bfb9638eec7debe3964e9d9931c78c15358a719c56d337
    let previous = generation::previous::previous_of(&planned.program, of_stem, &of_stem.outputs);
    let mut text = String::new();
    match &previous {
        Some(found) if !found.hash_matched => {}
        // @lfy def/mcp/main.lfy:changes#changes:changes:3b2a8142ec9ed2bb1f80958fc3261c1d1b4be77c99faa3057b834d1224905c31
        Some(_) => text.push_str(
            "the source its outputs were accepted from is not known; the changes below are against \
             the latest revision of its file with the same lowered code, so the criteria and tests \
             they report as changed may not be those that changed since its outputs were accepted\n",
        ),
        // @lfy def/mcp/main.lfy:changes#changes:changes:b2b31f75eb94528c8ec7526264ada5e72ab1d96bfb72f1d04697c832979b751d
        None => text.push_str(
            "neither the source its outputs were accepted from nor a revision of its file with the \
             same lowered code can be recovered; every entity is listed as added\n",
        ),
    }
    let found = generation::changes(&planned.program, of_stem, previous.as_ref()); // @lfy def/mcp/main.lfy:changes#changes:changes:76805f808eadb6afc31288f850876553118dd0066c28d603b5716c112369b09c
    if found.is_empty() {
        // @lfy def/mcp/main.lfy:changes#changes:changes:c91839604ea31b5f8cf98f38c9e887c17fa9d386f697dd83a406cd2307166b3d
        text.push_str(&format!("no changes in {unit} since its outputs were accepted"));
        return Ok(text);
    }
    // @lfy def/mcp/main.lfy:changes#changes:changes:76805f808eadb6afc31288f850876553118dd0066c28d603b5716c112369b09c
    let lines: Vec<String> = found
        .iter()
        .map(|change| format!("{} {} {}", change.entity, change.kind.as_str(), change.detail))
        .collect();
    text.push_str(&lines.join("\n"));
    Ok(text)
}
