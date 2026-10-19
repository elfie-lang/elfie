// @lfy def/mcp/compiling.lfy:task
//! The tools of a compiler run: its obligations, the interfaces of its dependencies, the
//! marker lines it asks the server to write, and how it says the run ended.

use super::*;

// Decision: a hosted compiler reads its obligations, its dependencies' interfaces, and its
// markers from the very request the compile handed it, so a compiler deep in a long run
// never reads definitions that may have changed since the compile planned.

/// What one compiler run is: the binding of its hosted session, and the request the compile
/// handed it.
// @lfy def/mcp/compiling.lfy:task#task:compilerTool:53be6121fea61b9a15629426fe7a361a2087f814c2a67975b1a428e724574063
struct Run<'s> {
    /// What the hosted session is bound to.
    binding: &'s Binding,
    /// The request its compiler run was given.
    request: &'s Request,
}

/// The run the session is bound to, and an error naming the tool when the session is bound
/// to no compiler run. Nothing is read or written when there is none.
// @lfy def/mcp/compiling.lfy:task#task:compilerTool:09099b01d8e84ae85a89b2a12642ec68e8d2b277dfec75f117a5e093caf818b2
// @lfy def/mcp/compiling.lfy:dependency#dependency:compilerTool:94e97e4b9416897e8f7296ad1449f08bf7cb435fe9a76fc4125ba6de8559c49e
// @lfy def/mcp/compiling.lfy:mark#mark:compilerTool:56146f12eb0eb11e3ccbcf6b89b563f1173e5c188a183687178892cdd6aefb62
// @lfy def/mcp/compiling.lfy:submit#submit:compilerTool:ee05d8be309e4a7aab71c6ed469b76e49a958abace52e8b6407087bf66c21140
fn own_request<'s>(session: &'s Session, tool: &str) -> Result<Run<'s>, String> {
    match &session.binding {
        Some(binding) => match &binding.request {
            // The unit acted on is the request's own, which is the binding's, and the outputs
            // are read and written under the binding's directory, which is the root the
            // hosted session's workspace was read at.
            // @lfy def/mcp/compiling.lfy:task#task:compilerTool:53be6121fea61b9a15629426fe7a361a2087f814c2a67975b1a428e724574063
            // @lfy def/mcp/compiling.lfy:task#task:compilerTool:a0f3f916620f9e289b39745855f3979f0a0db1a4ff2e30369ec791cd1d94087a
            // @lfy def/mcp/compiling.lfy:dependency#dependency:compilerTool:63c65ebb9e7de8e28dac04559c0480b93d42d1c8370c324dbc1f22d866a7d47e
            // @lfy def/mcp/compiling.lfy:dependency#dependency:compilerTool:0c15908a4dcfda7bb04d0631866acd9f199cf990a0bd8a108b4e4d2744608e3a
            // @lfy def/mcp/compiling.lfy:mark#mark:compilerTool:6f13a0aeb39746c3ebd7715af8dbde62ed2f84c5447cafb7ea41a17e8f34a300
            // @lfy def/mcp/compiling.lfy:mark#mark:compilerTool:85e5595e052eeca50b56f9cebdba26de23ab6e384826458c5f80bff35a6a3579
            // @lfy def/mcp/compiling.lfy:submit#submit:compilerTool:3b08f414dd244d49aaa1070d8e2a00ad42aac05ce340b2985b4829003e3cd0b7
            // @lfy def/mcp/compiling.lfy:submit#submit:compilerTool:fe5e191c412762eabc28bb250d5cdf400a97c9315325f022cff7b603d32ee079
            // @lfy def/mcp/compiling.lfy:reach#reach:compilerTool:e426f50710b23656e0aae598866def61ff7b6d8dcc0b2e6bf5d51c4ec2f76e6b
            // @lfy def/mcp/compiling.lfy:reach#reach:compilerTool:855fc01908f5e5df0d3cd77409a0c67eb73c60d0195f78608fecaf116ec59f78
            // @lfy def/mcp/compiling.lfy:touch#touch:compilerTool:e337d28709e9933b4c945f2487adb8f71f55f705eea530ddb0d15348006f71fc
            // @lfy def/mcp/compiling.lfy:touch#touch:compilerTool:ec9c9ad5e2855f806725f1c0f7a77436d443e8997b9f9edee0b84a7515ad8b7a
            Some(request) => Ok(Run { binding, request }),
            // @lfy def/mcp/compiling.lfy:dependency#dependency:compilerTool:94e97e4b9416897e8f7296ad1449f08bf7cb435fe9a76fc4125ba6de8559c49e
            // @lfy def/mcp/compiling.lfy:reach#reach:compilerTool:d77749f2f1a06bb69e87b5854ab4977a8efe3f853f774535bdca2b43444d57ae
            // @lfy def/mcp/compiling.lfy:touch#touch:compilerTool:cd6effeac8a136d2283bffd5d7a472f65a0f39326e6548f4a64585634acc8d52
            None => Err(format!("{tool} acts only in a session bound to a compiler run")),
        },
        // @lfy def/mcp/compiling.lfy:mark#mark:compilerTool:56146f12eb0eb11e3ccbcf6b89b563f1173e5c188a183687178892cdd6aefb62
        // @lfy def/mcp/compiling.lfy:submit#submit:compilerTool:ee05d8be309e4a7aab71c6ed469b76e49a958abace52e8b6407087bf66c21140
        // @lfy def/mcp/compiling.lfy:reach#reach:compilerTool:d77749f2f1a06bb69e87b5854ab4977a8efe3f853f774535bdca2b43444d57ae
        // @lfy def/mcp/compiling.lfy:touch#touch:compilerTool:cd6effeac8a136d2283bffd5d7a472f65a0f39326e6548f4a64585634acc8d52
        None => Err(format!("{tool} acts only in a session bound to a compiler run")),
    }
}

impl Run<'_> {
    /// The unit the run is for: the request's own, as an index into the binding's plan.
    // @lfy def/mcp/compiling.lfy:task#task:task:face0ae4233969eecd82ba1353a87659c587a519110a0bf39f5a40ab416dbaf8
    fn index(&self) -> usize {
        self.request.unit
    }

    /// The unit the run is for, as the binding's plan holds it.
    fn unit(&self) -> &Unit {
        &self.binding.plan.units[self.request.unit]
    }

    /// The units of the run's reach, as indices into the binding's plan, leaving out any the
    /// plan does not hold.
    fn reach(&self) -> Vec<usize> {
        self.request
            .reach
            .units
            .iter()
            .copied()
            .filter(|&index| self.binding.plan.units.get(index).is_some())
            .collect()
    }

    /// The stems of the units of the run's reach, joined, or the words saying this run may
    /// edit no other unit's code when there are none.
    fn reach_stems(&self) -> String {
        let stems: Vec<&str> = self.reach().into_iter().map(|index| self.binding.plan.units[index].stem.as_str()).collect();
        if stems.is_empty() {
            "this run may edit no other unit's code".to_string()
        } else {
            format!("the units this run may edit are: {}", stems.join(", "))
        }
    }
}

/// A name with any leading definition file path and colon set aside: the path, when one was
/// spelled, and what remains.
// @lfy def/mcp/compiling.lfy:dependency#dependency:dependency:0e9dc230cab50f2d56b0c3bb4b3df7405447b216f7d728bbdb5c789f693619a0
// @lfy def/mcp/compiling.lfy:mark#mark:mark:ea0e414361181918c63cd2a060bf3bdae8d34bdc0cc992fa89cac7fc18138b2f
fn without_path(name: &str) -> (Option<&str>, &str) {
    match name.rsplit_once(':') {
        Some((path, rest)) if path.ends_with(".lfy") => (Some(path), rest),
        _ => (None, name),
    }
}

/// What remains of a name split into the identifier it begins with and the member name that
/// follows the first dot, when one does.
// @lfy def/mcp/compiling.lfy:dependency#dependency:dependency:0e9dc230cab50f2d56b0c3bb4b3df7405447b216f7d728bbdb5c789f693619a0
// @lfy def/mcp/compiling.lfy:mark#mark:mark:ea0e414361181918c63cd2a060bf3bdae8d34bdc0cc992fa89cac7fc18138b2f
fn identifier_and_member(rest: &str) -> (&str, Option<&str>) {
    match rest.split_once('.') {
        Some((identifier, member)) => (identifier, Some(member)),
        None => (rest, None),
    }
}

/// Every entity of one file a marker may name, with the name it is named by: each
/// declaration of the file scope and each entity of `extra` the file declares, then the
/// members each of them declares as `owner.member`. The owner's own line comes after its
/// members', as acceptance builds the same table.
// @lfy def/mcp/compiling.lfy:task#task:task:0ba2a96c6162e5b2088f0d0af27dcf6dcd7a99b009bf3be3772ffa13ffa864f4
// @lfy def/mcp/compiling.lfy:mark#mark:mark:6e80b14bc0e6dc4b5716fc1c83947bb7088955e72a8f1b87f0f7f1014f4f2d48
fn named_in_file(model: &Model, source: FileId, extra: &[EntityId]) -> Vec<(String, EntityId)> {
    let mut declarations: Vec<EntityId> = Vec::new();
    if let Some(&scope) = model.file_scopes.get(source) {
        for symbol in model.scopes[scope].declared() {
            let symbol = &model.symbols[symbol];
            let record = &model.entities[symbol.entity];
            if symbol.kind != SymbolKind::Module
                && record.file == Some(source)
                && record.node.is_some()
                && !declarations.contains(&symbol.entity)
            {
                declarations.push(symbol.entity);
            }
        }
    }
    for &entity in extra {
        let record = &model.entities[entity];
        if record.node.is_some() && record.file == Some(source) && !declarations.contains(&entity) {
            declarations.push(entity);
        }
    }
    let mut out: Vec<(String, EntityId)> = Vec::new();
    for entity in declarations {
        let Some(identifier) = model.entities[entity].identifier.clone() else { continue };
        for symbol in model.members(entity) {
            let member = model.symbols[symbol].entity;
            if let Some(name) = &model.entities[member].identifier
                && model.entities[member].node.is_some()
            {
                out.push((format!("{identifier}.{name}"), member));
            }
        }
        out.push((identifier, entity));
    }
    out
}

/// Every entity a unit's criteria and tests may be for, with the name it is named by: those
/// of the unit's own file, then those of each file declaring an entity moved into it.
// @lfy def/mcp/compiling.lfy:task#task:task:0ba2a96c6162e5b2088f0d0af27dcf6dcd7a99b009bf3be3772ffa13ffa864f4
fn named_in_unit(model: &Model, workspace: &Workspace, unit: &Unit) -> Vec<(String, EntityId)> {
    let mut sources: Vec<FileId> = vec![workspace.files[unit.file].source];
    for &entity in &unit.entities {
        if let Some(source) = model.entities[entity].file
            && !sources.contains(&source)
        {
            sources.push(source);
        }
    }
    sources.iter().flat_map(|&source| named_in_file(model, source, &unit.entities)).collect()
}

/// A criterion's or test's place, as a review request spells it: the file of its origin
/// relative to the workspace root, a colon, and the line its origin begins on.
// @lfy def/mcp/compiling.lfy:task#task:task:48b5a17b6bd8bf11626b7e3c8a77f30620a9bcf76c647c3d0cb15aaa2ac3e582
fn origin_place(model: &Model, origin: NodeRef) -> String {
    let line = model.first_token(origin).map_or(1, |token| token.line);
    format!("{}:{line}", model.sources[origin.file].path)
}

/// One criterion or test of a request, read for a line of `elfie_task`.
// @lfy def/mcp/compiling.lfy:task#task:task:bc0cfc213f87dda7b8e1106191bb902bdb5a2b08b2472742020be295a621309b
struct Asked {
    /// Its id.
    id: String,
    /// The entity it is for; `None` when it is global.
    entity: Option<EntityId>,
    /// Where it is, as a review request spells a place.
    place: String,
    /// The text a request and a review show a reader.
    text: String,
}

/// One criterion or test of a request as its id, the entity it is for, its place, and the
/// text a request and a review show a reader.
// @lfy def/mcp/compiling.lfy:task#task:task:bc0cfc213f87dda7b8e1106191bb902bdb5a2b08b2472742020be295a621309b
// @lfy def/mcp/compiling.lfy:task#task:task:8bf5a91fa0df3686fb02ff5457cd19a9f3c4d034aceb88db292e7f850ecd6638
fn requirement_parts(model: &Model, requirement: &Requirement) -> Asked {
    match requirement {
        Requirement::Criterion(criterion) => Asked {
            id: criterion.id.clone(),
            entity: criterion.entity,
            place: origin_place(model, criterion.origin),
            text: criterion.text(),
        },
        // @lfy def/mcp/compiling.lfy:task#task:task:8bf5a91fa0df3686fb02ff5457cd19a9f3c4d034aceb88db292e7f850ecd6638
        Requirement::Test(test) => Asked {
            id: test.id.clone(),
            entity: test.entity,
            place: origin_place(model, test.origin),
            text: lowered_test_text(test),
        },
    }
}

/// The rewording of one criterion or test of a unit, when its id rewords one the unit's
/// existing outputs were generated against.
// @lfy def/mcp/compiling.lfy:task#task:task:d1e2bd77856838efb047c8fe5d6d27e38d190808ff140a2daf64c7cf550985f3
fn rewording_of<'r>(request: &'r Request, id: &str) -> Option<&'r generation::Rewording> {
    request.reworded.as_ref()?.iter().find(|rewording| rewording.id == id)
}

/// What a criterion or test of a unit is marked as: `reworded` when its id rewords one the
/// unit's existing outputs were generated against, `kept` when those outputs were generated
/// against it, `new` when they were generated against a known set that does not hold it or
/// there are no outputs yet, and `unknown` when what they answered is not recorded.
// @lfy def/mcp/compiling.lfy:task#task:task:f95499d17f549c43fa265f803f32119fa037bdad61a8ab6c7b710d1e19b92633
// @lfy def/mcp/compiling.lfy:task#task:task:97cb756cb2bee31d44637d2616d0097c1d221f7a5830d8b36a4adbec194b8bef
// @lfy def/mcp/compiling.lfy:task#task:task:e0ded60c07e56e8103c76b2a3e974f550ae276fc0206c0412f1a834c66cf0151
fn mark_of(request: &Request, has_outputs: bool, id: &str) -> &'static str {
    // A criterion that kept its place and changed its words took a new id, which the compile
    // has already moved its markers to, so it is marked apart from what is really new.
    // @lfy def/mcp/compiling.lfy:task#task:task:d1e2bd77856838efb047c8fe5d6d27e38d190808ff140a2daf64c7cf550985f3
    if rewording_of(request, id).is_some() {
        return "reworded";
    }
    match &request.answered {
        // @lfy def/mcp/compiling.lfy:task#task:task:f95499d17f549c43fa265f803f32119fa037bdad61a8ab6c7b710d1e19b92633
        Some(answered) if answered.iter().any(|known| known == id) => "kept",
        // @lfy def/mcp/compiling.lfy:task#task:task:97cb756cb2bee31d44637d2616d0097c1d221f7a5830d8b36a4adbec194b8bef
        Some(_) => "new",
        None if !has_outputs => "new",
        // @lfy def/mcp/compiling.lfy:task#task:task:e0ded60c07e56e8103c76b2a3e974f550ae276fc0206c0412f1a834c66cf0151
        None => "unknown",
    }
}

/// One edit another unit made inside a region of the run's unit, as a line: its kind, the
/// word by, the unit that made it, the word in, its path, a colon, the line its region
/// begins on, the entity the region is of, an em dash, and its note.
// @lfy def/mcp/compiling.lfy:task#task:task:e32fa38ec00ece223578e5d30201345f1f5e6104141d5c4635206132b68cfd99
fn foreign_edit_line(edit: &ForeignEdit) -> String {
    let entity = edit.region.entity.clone().unwrap_or_else(|| marker_text(&edit.region));
    format!(
        "{} by {} in {}:{} {entity} — {}",
        edit.kind.as_str(),
        edit.by,
        edit.path,
        edit.region.output_line,
        edit.note
    )
}

/// What this compiler run must do: its unit's outputs and reason, the edits other units made
/// to them, and every criterion and test it answers for.
// @lfy def/mcp/compiling.lfy:task#task:task:03c0e861c4bbd25a35c249a398a25887a28c2cef4fe08f743b831393be179644
pub fn task(session: &Session) -> Result<String, String> {
    let run = own_request(session, "elfie_task")?;
    let request = run.request;
    // Everything below is read from the request and from the program the compile lowered;
    // no definition file and no output is read.
    // @lfy def/mcp/compiling.lfy:task#task:task:03c0e861c4bbd25a35c249a398a25887a28c2cef4fe08f743b831393be179644
    let workspace = &run.binding.program.workspace;
    let model = &workspace.model;
    // @lfy def/mcp/compiling.lfy:task#task:task:face0ae4233969eecd82ba1353a87659c587a519110a0bf39f5a40ab416dbaf8
    let of_unit = run.unit();
    let has_outputs = !of_unit.outputs.is_empty();
    let file = &workspace.files[of_unit.file];
    let target = &workspace.targets[of_unit.target];
    let mut lines: Vec<String> = Vec::new();
    let (mut fresh, mut rephrased, mut kept, mut unknown, mut removed) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let reason = of_unit.reason.map_or_else(|| "up to date".to_string(), |reason| reason.as_str().to_string());
    // @lfy def/mcp/compiling.lfy:task#task:task:539f478257e58ce337bd56a59d32f6e3ac9e5dd1412f40177b504248b1030c39
    lines.push(format!("{} {} {reason}", of_unit.stem, file.path));
    if has_outputs {
        // @lfy def/mcp/compiling.lfy:task#task:task:950a71006d7f807babb9b09d7bc8cd3a42063f1ab0717e6b1b5006fe2031c452
        let outputs: Vec<&str> = of_unit.outputs.iter().map(|map| map.output.as_str()).collect();
        lines.push(format!("outputs {}", outputs.join(", ")));
    } else {
        // @lfy def/mcp/compiling.lfy:task#task:task:78f9e281986eff5db14a8f174b25fcbdd7a5c3c6d5c7357556c01dab5a977a04
        lines.push(format!(
            "no output yet; its outputs go under {} at the paths the guidance spells from the stem {}",
            target.output_directory, of_unit.stem
        ));
    }
    // @lfy def/mcp/compiling.lfy:task#task:task:05f4f2c0332db22eae294592d3b5235d10a9000d76d813ad235ab8a7a8c7706a
    // @lfy def/mcp/compiling.lfy:task#task:task:da9bc488fcbc7fe8be5448756d504a8b352588cfa6f0f6a3e1b93a460bd3df6b
    if !request.foreign_edits.is_empty() {
        lines.push("other units edited its outputs since it was last accepted:".to_string());
        lines.extend(request.foreign_edits.iter().map(foreign_edit_line));
    }
    let named = named_in_unit(model, workspace, of_unit);
    let own = workspace.files[of_unit.file].source;
    let mut headed: Vec<EntityId> = Vec::new();
    let mut answered: Vec<EntityId> = Vec::new();
    // @lfy def/mcp/compiling.lfy:task#task:task:42e538670c9d8018e287823eb07140bbb98c23acc8baeac8e3bcfb12b705b1e4
    for requirement in &request.requirements {
        let Asked { id, entity, place, text } = requirement_parts(model, requirement);
        if let Some(entity) = entity {
            if !answered.contains(&entity) {
                answered.push(entity);
            }
            // @lfy def/mcp/compiling.lfy:task#task:task:0ba2a96c6162e5b2088f0d0af27dcf6dcd7a99b009bf3be3772ffa13ffa864f4
            if !headed.contains(&entity) {
                headed.push(entity);
                let is_own_file = model.file_entities.get(own) == Some(&entity);
                let name = if is_own_file {
                    file.path.clone()
                } else {
                    named
                        .iter()
                        .find(|&&(_, candidate)| candidate == entity)
                        .map(|(name, _)| name.clone())
                        .unwrap_or_else(|| identifier_of(model, entity))
                };
                // An entity moved into the unit is declared in another file, which the line
                // names so that a marker for it spells that file.
                // @lfy def/mcp/compiling.lfy:task#task:task:a151226a2f7853ca59f5d23754f07954956c81793b8a0b4e771b88a500617003
                match model.entities[entity].file.filter(|&source| source != own) {
                    Some(source) => {
                        lines.push(format!("{name} declared in {}", model.sources[source].path));
                    }
                    None => lines.push(name),
                }
            }
        }
        let marked = mark_of(request, has_outputs, &id);
        match marked {
            "kept" => kept += 1,
            "unknown" => unknown += 1,
            "reworded" => rephrased += 1,
            _ => fresh += 1,
        }
        // @lfy def/mcp/compiling.lfy:task#task:task:bc0cfc213f87dda7b8e1106191bb902bdb5a2b08b2472742020be295a621309b
        lines.push(format!("{marked} {id} {place} — {text}"));
        // What a reworded one replaces, on the line after it.
        // @lfy def/mcp/compiling.lfy:task#task:task:d1e2bd77856838efb047c8fe5d6d27e38d190808ff140a2daf64c7cf550985f3
        if let Some(rewording) = rewording_of(request, &id) {
            lines.push(format!("replaces {} — {}", rewording.replaces, rewording.text));
        }
    }
    // Every entity of the unit must carry a marker, so those with nothing to answer for are
    // named: no criterion and no test of the unit is for them.
    // @lfy def/mcp/compiling.lfy:task#task:task:f9fb73337a13788bc2c97891ea0b6725150d8f585b1ea3c8102578a43fdc42d6
    let without: Vec<String> = of_unit
        .entities
        .iter()
        .filter(|entity| !answered.contains(entity))
        .map(|&entity| identifier_of(model, entity))
        .collect();
    if without.is_empty() {
        // @lfy def/mcp/compiling.lfy:task#task:task:8cda0fab4d2e20c30f36514faa7a119d7713f6defa2830d8a2284e0161739f87
        lines.push("no entity of the unit lacks a criterion or test".to_string());
    } else {
        // @lfy def/mcp/compiling.lfy:task#task:task:f9fb73337a13788bc2c97891ea0b6725150d8f585b1ea3c8102578a43fdc42d6
        lines.push(format!(
            "no criterion or test is for {}; each still needs a marker",
            without.join(", ")
        ));
    }
    match &request.removed {
        // @lfy def/mcp/compiling.lfy:task#task:task:359c526bf1661585212774516260b538e4f0ab4701c7e10f200e62855c18a363
        Some(gone) if !gone.is_empty() => {
            lines.push("these were removed since its outputs were accepted".to_string());
            for one in gone {
                removed += 1;
                // A removed criterion or test the request could not quote carries no text,
                // and no place either, since the revision that held it is not the one its
                // place would be read from; the line says so rather than quoting nothing.
                // @lfy def/mcp/compiling.lfy:task#task:task:66f11c93795ee4ea2822907afd33f054e3589b0287b19b0b618c695e86e32fbb
                if one.text.is_empty() {
                    lines.push(format!("removed {} — its text is not known", one.id));
                    continue;
                }
                // @lfy def/mcp/compiling.lfy:task#task:task:359c526bf1661585212774516260b538e4f0ab4701c7e10f200e62855c18a363
                lines.push(format!("removed {} {} — {}", one.id, one.place, one.text));
            }
        }
        // @lfy def/mcp/compiling.lfy:task#task:task:04aaa75f3b9f5d57d80f8170d92ad9358d6aab1ed98260a76078d0741d5aae54
        None if has_outputs => {
            lines.push("which criteria and tests were removed is not known".to_string());
        }
        _ => {}
    }
    // @lfy def/mcp/compiling.lfy:task#task:task:ca1abaa819942eb8c6c130e520f1c36ee16815c41b54d388cca6a9b8ad14a76c
    lines.push(format!(
        "{fresh} new, {rephrased} reworded, {kept} kept, {unknown} unknown, {removed} removed"
    ));
    Ok(lines.join("\n"))
}

/// The lines of one entity in the interface text of a unit: its own line, then the line of
/// each member or enum member it declares, or only the line of one member when one is named.
// @lfy def/mcp/compiling.lfy:dependency#dependency:dependency:5187f431c8e1a018407dfaf23c821f2cfc1f4a15370dc6acb8175a28e7fcd50c
// @lfy def/mcp/compiling.lfy:dependency#dependency:dependency:82fa16bd07081751c96e7033b5801a1698a74ccf133725c2d1e01ea9edd01680
fn interface_lines(text: &str, identifier: &str, member: Option<&str>) -> Vec<String> {
    let opening = format!("- `{identifier}` (");
    let mut found: Vec<String> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with(&opening) {
            inside = true;
            found.push(line.to_string());
            continue;
        }
        if !inside {
            continue;
        }
        let Some(rest) = line.strip_prefix("  ") else { break };
        // @lfy def/mcp/compiling.lfy:dependency#dependency:dependency:82fa16bd07081751c96e7033b5801a1698a74ccf133725c2d1e01ea9edd01680
        match member {
            Some(member) if !rest.starts_with(&format!("- `{member}` (")) => {}
            _ => found.push(line.to_string()),
        }
    }
    found
}

/// What one entity of a dependency of this compiler run's batch offers its dependents.
// @lfy def/mcp/compiling.lfy:dependency#dependency:dependency:88edd00efeadac6571b81f940a3b239335ff17aac9c416776584049ade3de935
pub fn dependency(session: &Session, name: &str) -> Result<String, String> {
    let run = own_request(session, "elfie_dependency")?;
    let (binding, request) = (run.binding, run.request);
    // The interfaces are the request's own, and the lines come from the program the compile
    // lowered; no definition file and no output is read.
    // @lfy def/mcp/compiling.lfy:dependency#dependency:dependency:88edd00efeadac6571b81f940a3b239335ff17aac9c416776584049ade3de935
    let plan = &binding.plan;
    let workspace = &binding.program.workspace;
    let model = &workspace.model;
    // @lfy def/mcp/compiling.lfy:dependency#dependency:dependency:0e9dc230cab50f2d56b0c3bb4b3df7405447b216f7d728bbdb5c789f693619a0
    let (in_file, rest) = without_path(name);
    let (identifier, member) = identifier_and_member(rest);
    let mut sections: Vec<String> = Vec::new();
    // @lfy def/mcp/compiling.lfy:dependency#dependency:dependency:48fa7258825a24866309481b9e6048acc49c37a1e74ed155a56a3210a884e862
    for interface in &request.interfaces {
        let Some(of_unit) = plan.units.get(interface.unit) else { continue };
        let file = &workspace.files[of_unit.file];
        // @lfy def/mcp/compiling.lfy:dependency#dependency:dependency:7b425db08ad081030e6137f0d6924c40b3b223d36d698551284b593335193519
        if in_file.is_some_and(|path| path != file.path) {
            continue;
        }
        // @lfy def/mcp/compiling.lfy:dependency#dependency:dependency:48fa7258825a24866309481b9e6048acc49c37a1e74ed155a56a3210a884e862
        if !interface
            .entities
            .iter()
            .any(|&entity| model.entities[entity].identifier.as_deref() == Some(identifier))
        {
            continue;
        }
        let text = generation::interface_of(workspace, of_unit);
        let found = interface_lines(&text, identifier, member);
        if found.is_empty() {
            continue;
        }
        // @lfy def/mcp/compiling.lfy:dependency#dependency:dependency:53420221788d685744cc63a541e15f79755a4b23efe12ec64289b0e9c90ad313
        let mut out = format!("{} {}\noutputs {}", of_unit.stem, file.path, interface.outputs.join(", "));
        // @lfy def/mcp/compiling.lfy:dependency#dependency:dependency:5187f431c8e1a018407dfaf23c821f2cfc1f4a15370dc6acb8175a28e7fcd50c
        for line in found {
            out.push('\n');
            out.push_str(&line);
        }
        sections.push(out);
    }
    if sections.is_empty() {
        // @lfy def/mcp/compiling.lfy:dependency#dependency:dependency:3d365d972122d9a5c77e7362aa0dec8a6f2e7fcc839899f595b9d15614a1d730
        return Err(format!(
            "no dependency of the unit declares {name}; elfie_entity reads an entity of the unit itself, or of any file"
        ));
    }
    // @lfy def/mcp/compiling.lfy:dependency#dependency:dependency:52155c3bef2ef807556011be184c10507617e56c46ebfded60f2e934d1a72699
    Ok(sections.join("\n\n---\n\n"))
}

// ---- the later units a compiler run may edit ----------------------------------------

// Decision: a run may edit every later unit of its target, which for an early unit is most of
// the program, so elfie_reach lists every unit of the reach in one line each but only the
// regions the run has edited, and gives every region of one unit when the caller names it.
// Decision: the cap left is measured against the outputs as they were before the run, never
// against the last call, so it says what acceptance will count, however many edits made it.

/// The stub line of a target: the value the setters of its declaration give the member
/// `stubLine` of `targetLanguage`; `None` when none gives it one, and then no edit is a stub.
fn stub_line_of(model: &Model, target: &Target) -> Option<String> {
    model.entities[target.declaration]
        .value("stubLine")
        .and_then(model::Value::as_str)
        .map(str::to_string)
}

/// The lines of one region of a text, the marker's own line first, each without its line
/// break.
fn region_lines(lines: &[&str], marker: &Marker) -> Vec<String> {
    let last = marker.end.min(lines.len());
    let first = marker.output_line.saturating_sub(1).min(last);
    lines[first..last].iter().map(|line| line.to_string()).collect()
}

/// One region of an output of a unit of a run's reach, and what the run has done to it.
struct ReachRegion {
    /// The output file it is in, relative to the root.
    output: String,
    /// The marker that begins it.
    marker: Marker,
    /// Whether the run edited it, and how: `None` when it reads as it did before the run.
    kind: Option<ForeignEditKind>,
    /// The lines the run's edit changes in it, as a foreign edit counts them.
    changed: usize,
}

/// Every region of the unit at an index of the run's plan: the marker of each of its own
/// regions its output files hold when the call arrives, read as [`generation::rederive`]
/// reads them, in output then line order, each with what the run has done to it.
///
/// A region is edited when its lines differ from those of the region with the same marker —
/// the same spelling, the same place among the markers spelled alike — in the file of the
/// binding's `before` at its path, or when that file has no such region. Its kind is stub when
/// it only keeps lines it had and adds stub lines naming the run's unit, and fix otherwise.
// @lfy def/mcp/compiling.lfy:reach#reach:reach:6e08c2c858ffbe279c640710e6c84d3e359af0f54110f8861822777f9274ec4f
fn reach_regions(session: &Session, run: &Run<'_>, index: usize) -> Vec<ReachRegion> {
    let binding = run.binding;
    let program = program_at_root(session, binding);
    let workspace = &program.workspace;
    let unit = &binding.plan.units[index];
    let claimed = sources_of(workspace, unit);
    let stub_line = workspace.targets.get(unit.target).and_then(|target| stub_line_of(&workspace.model, target));
    let before: &[Output] = binding.before.as_deref().unwrap_or(&[]);
    let mut found: Vec<ReachRegion> = Vec::new();
    for map in generation::rederive(&program, &binding.plan, index, &unit.outputs) {
        let now = text_now(session, &map.output).unwrap_or_default();
        let now_lines: Vec<&str> = now.lines().collect();
        let was = text_in(before, &map.output);
        let was_lines: Vec<&str> = was.map(|text| text.lines().collect()).unwrap_or_default();
        let was_markers: Vec<Marker> = was.map(generation::parse_markers).unwrap_or_default();
        let mut own: Vec<&Marker> = map.markers.iter().filter(|marker| claimed.contains(&marker.file)).collect();
        own.sort_by_key(|marker| marker.output_line);
        for marker in own {
            let spelled = marker_text(marker);
            let place = map
                .markers
                .iter()
                .filter(|other| other.output_line < marker.output_line && marker_text(other) == spelled)
                .count();
            let after = region_lines(&now_lines, marker);
            // @lfy def/mcp/compiling.lfy:reach#reach:reach:6a3be6fd6b463b81cecca24ba4e9380ba6d077e674b64d3ae8e9769bccf78a3e
            let prior: Option<Vec<String>> = was_markers
                .iter()
                .filter(|other| marker_text(other) == spelled)
                .nth(place)
                .map(|other| region_lines(&was_lines, other));
            let (kind, changed) = match prior {
                Some(prior) if prior == after => (None, 0),
                prior => {
                    let prior = prior.unwrap_or_default();
                    // @lfy def/mcp/compiling.lfy:reach#reach:reach:ab06b81d85917c4b689e3f2d880551c159433da1a74db73a25d2260e2d773939
                    let stubbed = stub_line
                        .as_deref()
                        .is_some_and(|stub_line| generation::is_stub_edit(&prior, &after, &run.unit().stem, stub_line));
                    let kind = if stubbed { ForeignEditKind::Stub } else { ForeignEditKind::Fix };
                    (Some(kind), generation::changed_lines(&prior, &after))
                }
            };
            found.push(ReachRegion {
                output: map.output.clone(),
                marker: marker.clone(),
                kind,
                changed,
            });
        }
    }
    found
}

/// How a count stands against a cap: what is left of it, or by how much the count exceeds it.
fn left_of(count: usize, cap: usize) -> String {
    if count > cap {
        format!("{} over", count - cap)
    } else {
        format!("{} left", cap - count)
    }
}

/// One region as a line: indented by two spaces, its output path, a colon, the lines it
/// spans, its entity and the requirement its marker names, then what the run did to it.
// @lfy def/mcp/compiling.lfy:reach#reach:reach:19b0c5cf0f5bb533a9f328f2c04a6d3044b1b3e0443c0c4a1ca3ae6ea8144e0a
fn reach_region_line(region: &ReachRegion, fix_lines: usize) -> String {
    let marker = &region.marker;
    let mut line = format!(
        "  {}:{}-{} {}",
        region.output,
        marker.output_line,
        marker.end,
        marker.entity.clone().unwrap_or_else(|| marker_text(marker))
    );
    if let Some(requirement) = &marker.requirement {
        line.push_str(&format!(" {requirement}"));
    }
    match region.kind {
        // @lfy def/mcp/compiling.lfy:reach#reach:reach:8e1fa030cbe98f1b5ead1cabbe92371b6dd678d06b86a6c15f37a1c89aa05248
        Some(ForeignEditKind::Stub) => line.push_str(" stub"),
        // @lfy def/mcp/compiling.lfy:reach#reach:reach:2352621fb062f56052c07817412ee289e45facefad7c707279cec577ce229177
        Some(ForeignEditKind::Fix) => line.push_str(&format!(
            " fix {} of {fix_lines}, {}",
            region.changed,
            left_of(region.changed, fix_lines)
        )),
        // @lfy def/mcp/compiling.lfy:reach#reach:reach:cbeeb6ddf0c9f41ed85f615e25b724bd99960f1a53cee611897aa4b688c885ca
        None => line.push_str(" unedited"),
    }
    line
}

/// The later units whose code this compiler run may edit, the regions of their outputs, and
/// what is left of the fix cap for each region and each unit, given the outputs on disk now.
///
/// Nothing is written, and no definition file is read.
// @lfy def/mcp/compiling.lfy:reach#reach:reach:ad6d0d87c88b3fa751fa9563db0ec0c99a3c4dcf96ae2e79f22fdd322e736908
// @lfy def/mcp/compiling.lfy:reach#reach:reach:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
pub fn reach(session: &Session, unit: Option<&str>) -> Result<String, String> {
    let run = own_request(session, "elfie_reach")?;
    let cap = &run.request.reach;
    let units = run.reach();
    let plan = &run.binding.plan;
    let workspace = &run.binding.program.workspace;
    let touches = session.touched();
    let (chosen, every_region): (Vec<usize>, bool) = match unit {
        Some(stem) => match units.iter().find(|&&index| plan.units[index].stem == stem) {
            Some(&index) => (vec![index], true),
            // @lfy def/mcp/compiling.lfy:reach#reach:reach:612c79a0bd570cca3b15daca7e28a4d15edd71b75289ef0817a9d8e1bcea025a
            None => return Err(format!("{stem} is no unit of this run's reach; {}", run.reach_stems())),
        },
        None => (units.clone(), false),
    };
    if units.is_empty() {
        // @lfy def/mcp/compiling.lfy:reach#reach:reach:ddc6993419dd1198ca5bbbfbd9ca03ed5cf8dd47101df0b07e9f9d64ad5668c2
        return Ok(
            "this run may edit no other unit's code, so every line it changes is in its own regions or in a shared output"
                .to_string(),
        );
    }
    // @lfy def/mcp/compiling.lfy:reach#reach:reach:88bcc79197d799f1c8d800606697b6c0f5cbc7e8119c081ab62717e33251d9a8
    let mut lines: Vec<String> = vec![format!(
        "a fix may change {} lines in one region and the fixes {} lines in one unit; a bigger edit must be a stub, and every unit with a fix needs a note through elfie_touch",
        cap.fix_lines, cap.fix_unit_lines
    )];
    // @lfy def/mcp/compiling.lfy:reach#reach:reach:236119a1c13c154015a99008f37e6978614d44b922e615904a7e44ec0a907c30
    // @lfy def/mcp/compiling.lfy:reach#reach:reach:1037292809b87ea03318bf0710ef237ebc0505c0c531acddcf63039f7c21370e
    for index in chosen {
        let of_unit = &plan.units[index];
        let regions = reach_regions(session, &run, index);
        let fixed: Vec<&ReachRegion> =
            regions.iter().filter(|region| region.kind == Some(ForeignEditKind::Fix)).collect();
        let changed: usize = fixed.iter().map(|region| region.changed).sum();
        // @lfy def/mcp/compiling.lfy:reach#reach:reach:c13bcc7c5ac7bce345fd001c67d733bf56d5c4ba20a0f0a17f12693cbdcbf4cd
        let mut line = format!(
            "{} {} {changed} of {}, {}",
            of_unit.stem,
            workspace.files[of_unit.file].path,
            cap.fix_unit_lines,
            left_of(changed, cap.fix_unit_lines)
        );
        match touches.get(&of_unit.stem) {
            // @lfy def/mcp/compiling.lfy:reach#reach:reach:ab3be2e89dbc4fa27deac091973926fcda512b1fc063a631f089cfac2ddad105
            Some(note) => line.push_str(&format!("; note: {note}")),
            // @lfy def/mcp/compiling.lfy:reach#reach:reach:dbdf60fafa6bd921c7b019f16bdfdfae6d7b219467ff6679f2dfad86f1b649d3
            None if !fixed.is_empty() => line.push_str("; needs a note through elfie_touch"),
            None => {}
        }
        lines.push(line);
        for region in regions.iter().filter(|region| every_region || region.kind.is_some()) {
            lines.push(reach_region_line(region, cap.fix_lines));
        }
    }
    Ok(lines.join("\n"))
}

/// Say why this compiler run edited the code of a later unit: the note is kept in the session
/// for that unit, in place of any it was given earlier, until the CLI closes the session.
///
/// Nothing is written.
// @lfy def/mcp/compiling.lfy:touch#touch:touch:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
pub fn touch(session: &Session, unit: &str, note: &str) -> Result<String, String> {
    let run = own_request(session, "elfie_touch")?;
    let plan = &run.binding.plan;
    // @lfy def/mcp/compiling.lfy:touch#touch:touch:3ad8f735544ed9fff63d14702b492f2d67c0766b719e25fd3fbb8be1723c2b20
    if !run.reach().iter().any(|&index| plan.units[index].stem == unit) {
        return Err(format!(
            "{unit} is no unit of this run's reach, so no note is recorded; {}",
            run.reach_stems()
        ));
    }
    // @lfy def/mcp/compiling.lfy:touch#touch:touch:504b1a8b9cf3534b61ea07b959247e1f7b3e2aa08e0ea6413dca004e3b20ba39
    if note.trim().is_empty() {
        return Err("a note says why the unit's code was edited; nothing is recorded".to_string());
    }
    // @lfy def/mcp/compiling.lfy:touch#touch:touch:d47f9dd713936933b35c0217763a11963a0f0a3c340f0ba8d9c47440cb84fd70
    if note.contains(['\n', '\r']) {
        return Err("a note is one line; nothing is recorded".to_string());
    }
    // @lfy def/mcp/compiling.lfy:touch#touch:touch:89f37d52cf86059d5d6deb84d257f9f2eb6205714c76d832c2e94f430133e27b
    session
        .touches
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(unit.to_string(), note.to_string());
    // @lfy def/mcp/compiling.lfy:touch#touch:touch:375335b7467058d45a48410b8ae739752a4dfef36ba775f028524a965bc45dfb
    Ok(format!(
        "recorded the note for {unit}; it is the note of every fix this run makes in the code of {unit}, and a later call of elfie_touch for {unit} replaces it"
    ))
}

// ---- the markers of a compiler run --------------------------------------------------

// Decision: an id is 64 hex digits and more, which an agent copies wrong often enough to
// matter, so the server writes a marker's id from one it has checked, and moves every marker
// of a reworded criterion to its new id in one call.

/// The source paths a marker of one of a unit's regions may name: the unit's file, and the
/// file of each entity moved into it.
// @lfy def/mcp/compiling.lfy:mark#mark:mark:50f18df59546947552b52843db51a0eab9607fe330a79b7c11e98da7f2cbc7fa
fn sources_of(workspace: &Workspace, unit: &Unit) -> Vec<String> {
    let model = &workspace.model;
    let mut found: Vec<String> = vec![workspace.files[unit.file].path.clone()];
    for &entity in &unit.entities {
        if let Some(source) = model.entities[entity].file
            && !found.contains(&model.sources[source].path)
        {
            found.push(model.sources[source].path.clone());
        }
    }
    found
}

/// The source paths a marker of one of the run's own regions may name.
// @lfy def/mcp/compiling.lfy:mark#mark:mark:50f18df59546947552b52843db51a0eab9607fe330a79b7c11e98da7f2cbc7fa
fn own_sources(run: &Run<'_>) -> Vec<String> {
    sources_of(&run.binding.program.workspace, run.unit())
}

/// The target of the unit of a compiler run, read off the session's workspace so that its
/// output directory is read under the binding's directory.
// @lfy def/mcp/compiling.lfy:mark#mark:mark:52dcea5ba1304502d225b631249c459cecf128b5f17ded9a63a98e62ef6c62bf
fn own_target<'w>(workspace: &'w Workspace, run: &Run<'_>) -> Option<&'w Target> {
    workspace.targets.get(run.unit().target)
}

/// Whether one id is that of a local criterion or test of the run's unit.
// @lfy def/mcp/compiling.lfy:mark#mark:mark:5bbb75b2e68dfacdf922b33253428ff59e3f168016b98840f11769f1c56a3553
fn is_local_requirement(request: &Request, id: &str) -> bool {
    request.requirements.iter().any(|one| one.id() == id)
}

/// Whether one id is that of a local criterion or test of the run's unit for one entity.
// @lfy def/mcp/compiling.lfy:mark#mark:mark:dd3ef6b416a23f341639e29ff2f36569d059617f0c4d6c5404dea75abb9f0ef2
fn is_requirement_of(request: &Request, id: &str, entity: EntityId) -> bool {
    request.requirements.iter().any(|one| {
        let of = match one {
            Requirement::Criterion(criterion) => criterion.entity,
            Requirement::Test(test) => test.entity,
        };
        one.id() == id && of == Some(entity)
    })
}

/// Whether one id is that of a global criterion or test of the request.
// @lfy def/mcp/compiling.lfy:mark#mark:mark:dd3ef6b416a23f341639e29ff2f36569d059617f0c4d6c5404dea75abb9f0ef2
fn is_global_requirement(request: &Request, id: &str) -> bool {
    request.globals.iter().any(|one| one.id() == id)
}

/// Rewrite to a new id every marker of this run's outputs that names an old one.
///
/// Which markers those are is [`generation::rename`]'s to decide, and it decides it as
/// acceptance does: only the markers naming the unit's file, or a file declaring an entity
/// moved into it, so another unit's markers are never touched.
/// The outputs it returns are the ones whose markers it rewrote, and only those are written.
// @lfy def/mcp/compiling.lfy:mark#mark:mark:4f1164ac3a94d643b82a1b08eb15af6e67163154209b7f063c857c86b7238356
fn rename_markers(
    session: &Session,
    run: &Run<'_>,
    target: &Target,
    id: &str,
    replaces: &str,
) -> Result<String, String> {
    let workspace = &session.workspace;
    let binding = run.binding;
    // @lfy def/mcp/compiling.lfy:mark#mark:mark:5bbb75b2e68dfacdf922b33253428ff59e3f168016b98840f11769f1c56a3553
    if !is_local_requirement(run.request, id) {
        return Err(format!(
            "{id} is no criterion or test of the unit {}; elfie_task lists them",
            run.unit().stem
        ));
    }
    let index = run.index();
    // The output files of the unit's target as they are when the call arrives.
    // @lfy def/mcp/compiling.lfy:mark#mark:mark:1a530e21d2168ea23ea4022f14c67e29613acca0bccddaf0f5e246bc1bbad43a
    let mut read: Vec<Output> = Vec::new();
    for output in output_files(workspace, target) {
        let path = workspace.root.join(&output);
        let Ok(text) = fs::read_to_string(&path) else { continue };
        read.push(Output { path: output, text });
    }
    let ids = BTreeMap::from([(replaces.to_string(), id.to_string())]);
    // @lfy def/mcp/compiling.lfy:mark#mark:mark:1a530e21d2168ea23ea4022f14c67e29613acca0bccddaf0f5e246bc1bbad43a
    let renamed = generation::rename(workspace, &binding.plan, index, &read, &ids);
    let mut rewritten: Vec<String> = Vec::new();
    for output in &renamed {
        // @lfy def/mcp/compiling.lfy:mark#mark:mark:1a530e21d2168ea23ea4022f14c67e29613acca0bccddaf0f5e246bc1bbad43a
        let path = workspace.root.join(&output.path);
        fs::write(&path, &output.text)
            .map_err(|error| format!("{} cannot be written: {error}", output.path))?;
        // The lines that differ from the file as it was read are the rewritten markers, since
        // `rename` keeps every other line of an output it returns.
        // @lfy def/mcp/compiling.lfy:mark#mark:mark:4f1164ac3a94d643b82a1b08eb15af6e67163154209b7f063c857c86b7238356
        let before: Vec<&str> = read
            .iter()
            .find(|one| one.path == output.path)
            .map(|one| one.text.split_inclusive('\n').collect())
            .unwrap_or_default();
        let after = markers_of(&output.text);
        for (at, line) in output.text.split_inclusive('\n').enumerate() {
            if before.get(at).is_some_and(|was| *was == line) {
                continue;
            }
            // @lfy def/mcp/compiling.lfy:mark#mark:mark:5bb1f1250f7f259cd65c04fd3be30c251cc135967710c7ce195a78f31866f8fe
            for marker in markers_on(&after, at + 1) {
                rewritten.push(format!("{}:{} {}", output.path, at + 1, marker_text(marker)));
            }
        }
    }
    if rewritten.is_empty() {
        // @lfy def/mcp/compiling.lfy:mark#mark:mark:95861eab55aed3f41b6a4bdab75d4e18ff5a4716d3a6751ca86c64341f33e296
        return Err(format!(
            "no marker of the output files of the unit's target {} naming {} names {replaces}",
            target.identifier,
            workspace.files[binding.plan.units[index].file].path
        ));
    }
    // @lfy def/mcp/compiling.lfy:mark#mark:mark:5bb1f1250f7f259cd65c04fd3be30c251cc135967710c7ce195a78f31866f8fe
    let mut text = format!("{} marker{} rewritten", rewritten.len(), if rewritten.len() == 1 { "" } else { "s" });
    for line in rewritten {
        text.push('\n');
        text.push_str(&line);
    }
    Ok(text)
}

/// The entity one name picks out of the run's unit, with the name it is named by and the
/// path of the file that declares it.
// @lfy def/mcp/compiling.lfy:mark#mark:mark:6e80b14bc0e6dc4b5716fc1c83947bb7088955e72a8f1b87f0f7f1014f4f2d48
fn entity_named(run: &Run<'_>, entity: &str) -> Result<(String, String, EntityId), String> {
    let workspace = &run.binding.program.workspace;
    let model = &workspace.model;
    // @lfy def/mcp/compiling.lfy:mark#mark:mark:ea0e414361181918c63cd2a060bf3bdae8d34bdc0cc992fa89cac7fc18138b2f
    let (in_file, rest) = without_path(entity);
    let (identifier, member) = identifier_and_member(rest);
    let mut found: Vec<(String, String, EntityId)> = Vec::new();
    for &candidate in &run.unit().entities {
        if model.entities[candidate].identifier.as_deref() != Some(identifier) {
            continue;
        }
        let Some(source) = model.entities[candidate].file else { continue };
        let path = model.sources[source].path.clone();
        if in_file.is_some_and(|named| named != path) {
            continue;
        }
        // @lfy def/mcp/compiling.lfy:mark#mark:mark:6e80b14bc0e6dc4b5716fc1c83947bb7088955e72a8f1b87f0f7f1014f4f2d48
        let named = match member {
            Some(member) => match model
                .members(candidate)
                .into_iter()
                .find(|&symbol| model.symbols[symbol].name == member)
            {
                Some(symbol) => model.symbols[symbol].entity,
                None => continue,
            },
            None => candidate,
        };
        let name = match member {
            Some(member) => format!("{identifier}.{member}"),
            None => identifier.to_string(),
        };
        if !found.iter().any(|(_, spelled, _)| spelled == &path) {
            found.push((name, path, named));
        }
    }
    match found.len() {
        // @lfy def/mcp/compiling.lfy:mark#mark:mark:af478fe84c65f8f3a8750f1acb30f2dbc8adcb4173e9a28f06be3b4389f86287
        0 => Err(format!("the unit {} does not declare {entity}", run.unit().stem)),
        1 => Ok(found.remove(0)),
        // @lfy def/mcp/compiling.lfy:mark#mark:mark:a38062a1fcda5efa808baa27f8d58d69ab0670d6640e713cece4075635abc240
        _ => {
            let files: Vec<&str> = found.iter().map(|(_, path, _)| path.as_str()).collect();
            Err(format!(
                "{entity} names entities of more than one file ({}); a file path and a colon before the name chooses one",
                files.join(", ")
            ))
        }
    }
}

/// Write one marker line into one of this run's outputs.
// @lfy def/mcp/compiling.lfy:mark#mark:mark:e959da5d59df47ac7933bb07c057234854cf20b78a1ae1296c4d2521057c5f32
fn insert_marker(
    session: &Session,
    run: &Run<'_>,
    target: &Target,
    id: Option<&str>,
    file: &str,
    line: f64,
    entity: &str,
) -> Result<String, String> {
    let workspace = &session.workspace;
    let binding = run.binding;
    let asked = line;
    // @lfy def/mcp/compiling.lfy:mark#mark:mark:096f3a8e4c7a62fc8b97a432770445c9c9061225f9849d6125699bff1233aa64
    if !output_files(workspace, target).iter().any(|known| known == file) {
        return Err(format!(
            "{file} is not an output file of the unit's target {}; write the output first, then insert a marker into it",
            target.identifier
        ));
    }
    let path = workspace.root.join(file);
    let text = fs::read_to_string(&path).map_err(|error| format!("{file} cannot be read: {error}"))?;
    let count = text.lines().count();
    // @lfy def/mcp/compiling.lfy:mark#mark:mark:a86c8a8dd444a7e09d183e1c131c3e4e0e74fdc0a4a752698290645da729a4b0
    // A line that is no whole number is no line of the file either.
    if asked.fract() != 0.0 || asked < 1.0 || asked > (count + 1) as f64 {
        return Err(format!(
            "{file} has {count} lines, so line must be a whole number between 1 and {}, not {asked}",
            count + 1
        ));
    }
    let line = asked as usize;
    // The last marker before the line says whose region the line falls in; a region of
    // another unit is never written into.
    // @lfy def/mcp/compiling.lfy:mark#mark:mark:50f18df59546947552b52843db51a0eab9607fe330a79b7c11e98da7f2cbc7fa
    let markers = markers_of(&text);
    let allowed = own_sources(run);
    if let Some(before) = markers.iter().rev().find(|marker| marker.output_line < line)
        && !allowed.contains(&before.file)
    {
        let owner = binding
            .plan
            .units
            .iter()
            .find(|of_unit| workspace.files[of_unit.file].path == before.file)
            .map(|of_unit| format!(", the file of the unit {}", of_unit.stem))
            .unwrap_or_default();
        return Err(format!(
            "line {line} of {file} falls in a region of another unit: its marker names {}{owner}",
            before.file
        ));
    }
    let (name, declaring, named) = entity_named(run, entity)?;
    // @lfy def/mcp/compiling.lfy:mark#mark:mark:dd3ef6b416a23f341639e29ff2f36569d059617f0c4d6c5404dea75abb9f0ef2
    if let Some(id) = id
        && !is_requirement_of(run.request, id, named)
        && !is_global_requirement(run.request, id)
    {
        return Err(format!(
            "{id} is no criterion or test of {name} in the unit {} and no global one; elfie_task lists them",
            run.unit().stem
        ));
    }
    let mut pieces: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
    // The line break is the one the file's first line ends with, and a line feed when it has
    // none.
    // @lfy def/mcp/compiling.lfy:mark#mark:mark:f81ece6f1d736c69189687f1b61d0a5d106ce51765776bbd8523caaecef41dea
    let ending = match pieces.first() {
        Some(first) if first.ends_with("\r\n") => "\r\n",
        _ => "\n",
    };
    // @lfy def/mcp/compiling.lfy:mark#mark:mark:e959da5d59df47ac7933bb07c057234854cf20b78a1ae1296c4d2521057c5f32
    let indent: String = match pieces.get(line.saturating_sub(1)).filter(|_| line <= count) {
        Some(at) => at.chars().take_while(|&character| character == ' ' || character == '\t').collect(),
        None => String::new(),
    };
    let inserted = match id {
        Some(id) => format!("{indent}{} @lfy {declaring}:{name}#{id}", target.marker_comment),
        None => format!("{indent}{} @lfy {declaring}:{name}", target.marker_comment),
    };
    if line > count {
        // @lfy def/mcp/compiling.lfy:mark#mark:mark:0d151705612442f586374d3ecc64db058c44120fe7a7355be4a84f2aafe6d259
        if let Some(last) = pieces.last_mut()
            && !last.ends_with('\n')
        {
            last.push_str(ending);
        }
        pieces.push(format!("{inserted}{ending}"));
    } else {
        // @lfy def/mcp/compiling.lfy:mark#mark:mark:f81ece6f1d736c69189687f1b61d0a5d106ce51765776bbd8523caaecef41dea
        pieces.insert(line - 1, format!("{inserted}{ending}"));
    }
    fs::write(&path, pieces.concat()).map_err(|error| format!("{file} cannot be written: {error}"))?;
    let at = line.min(count + 1);
    // @lfy def/mcp/compiling.lfy:mark#mark:mark:2e402e1e9ad7b4d3ccb7a990b41387cfa8be2150d5feaa6d160ee8adfd91704e
    Ok(format!("{file}:{at} {inserted}"))
}

/// Write a marker line into one of this compiler run's outputs, or rewrite to a new id every
/// marker of its outputs that names an old one, so that no id is typed by hand.
///
/// Nothing but an output file of the unit's target is written: no source map and no review
/// record, which the CLI writes, and no file outside that target's output directory.
// @lfy def/mcp/compiling.lfy:mark#mark:mark:af3f47a174d2ca2377099af56a6a2784a84ff3e0f957c1bfffcab64de732a8a2
// @lfy def/mcp/compiling.lfy:mark#mark:mark:fcfecab8991108e6f27911bb4b6d77cfffcf93ddbb785581af7a1021cbe3b53c
pub fn mark(
    session: &Session,
    id: Option<&str>,
    replaces: Option<&str>,
    file: Option<&str>,
    line: Option<f64>,
    entity: Option<&str>,
) -> Result<String, String> {
    let run = own_request(session, "elfie_mark")?;
    let Some(target) = own_target(&session.workspace, &run) else {
        return Err(format!("the unit {} has no target", run.unit().stem));
    };
    // The arguments a call gives say what it wants: `replaces` asks for a rename, and `file`,
    // `line`, and `entity` ask for an insertion.
    // @lfy def/mcp/compiling.lfy:mark#mark:mark:de06da3d9e51338cb41d6032e36580ce6510b4297822e054fb6f234dc68b0594
    if let Some(replaces) = replaces {
        if id.is_none() || file.is_some() || line.is_some() || entity.is_some() {
            return Err(
                "a rename takes replaces and id and nothing else; leave out file, line, and entity".to_string()
            );
        }
        let id = id.unwrap_or_default();
        return rename_markers(session, &run, target, id, replaces);
    }
    // @lfy def/mcp/compiling.lfy:mark#mark:mark:2959e7495b60ed84e030a118eff9b9a1cb5d699815713975e3e7f53137050e14
    let (Some(file), Some(line), Some(entity)) = (file, line, entity) else {
        return Err("an insertion takes file, line, and entity, and optionally id".to_string());
    };
    insert_marker(session, &run, target, id, file, line, entity)
}

// ---- how a compiler run ended -------------------------------------------------------

// Decision: the compiler's outcome is submitted while it is still working, checked when it
// arrives, and kept in its session for the CLI, so how a run ended never depends on the last
// lines of a final message.

/// The line a compiler's final message ends with for each outcome.
// @lfy def/mcp/compiling.lfy:submit#submit:submit:f94daa1287cb9c2fd43bbdfe11f8cfe51bf9706696102840706f0445f387669a
const ENDS_WITH: &str = "ELFIE: DONE, ELFIE: BLOCKED: and the reason, or ELFIE: CLARIFY: and the question";

/// Say how this compiler run ended. Nothing is written: the outcome is kept in the session
/// until the CLI closes it.
// @lfy def/mcp/compiling.lfy:submit#submit:submit:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
pub fn submit(session: &Session, outcome: &str, message: &str) -> Result<String, String> {
    own_request(session, "elfie_submit")?;
    // @lfy def/mcp/compiling.lfy:submit#submit:submit:28e49a28fbbfa8f85d4d95ecccb6f05ae599e3a6f128b655d73257666a31412e
    let Some(kind) = SubmittedOutcome::from_name(&outcome.to_ascii_lowercase()) else {
        return Err(format!(
            "the outcome {outcome} is none of {}, {}, and {}",
            SubmittedOutcome::Done.as_str(),
            SubmittedOutcome::Blocked.as_str(),
            SubmittedOutcome::Clarify.as_str()
        ));
    };
    // @lfy def/mcp/compiling.lfy:submit#submit:submit:b99c292c6a953720125d01484c7cc9fc03b9727847086a54737c3363884273cb
    if matches!(kind, SubmittedOutcome::Blocked | SubmittedOutcome::Clarify) && message.trim().is_empty() {
        return Err(format!(
            "{} needs its reason and {} its question",
            SubmittedOutcome::Blocked.as_str(),
            SubmittedOutcome::Clarify.as_str()
        ));
    }
    // @lfy def/mcp/compiling.lfy:submit#submit:submit:839e3605860269cb5d2a142eae47825edd5f8e18cc86e1a3b9c4f70278331d8d
    let mut recorded = session.submission.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *recorded = Some(Submission {
        outcome: kind,
        message: message.to_string(),
    });
    // @lfy def/mcp/compiling.lfy:submit#submit:submit:bedeb2634345b2348c1509d268083160d2e9755e210b5107f4fbc972522749c1
    // @lfy def/mcp/compiling.lfy:submit#submit:submit:f94daa1287cb9c2fd43bbdfe11f8cfe51bf9706696102840706f0445f387669a
    Ok(format!(
        "recorded the outcome {}; a later call of elfie_submit replaces it\nend your final message with the one line for the outcome: {ENDS_WITH}",
        kind.as_str()
    ))
}
