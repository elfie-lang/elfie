// @lfy def/mcp/main.lfy:task
//! The tools of a compiler run: its obligations, the interfaces of its dependencies, the
//! marker lines it asks the server to write, and how it says the run ended.

use super::*;

// Decision: a hosted compiler reads its obligations, its dependencies' interfaces, and its
// markers from the very request the compile handed it, so a compiler deep in a long run
// never reads definitions that may have changed since the compile planned.

/// What one compiler run is: the binding of its hosted session, and the request the compile
/// handed it.
// @lfy def/mcp/main.lfy:task#task:compilerTool:1e78463366ba8fbbfbec5fc142e181f67f7b2aead81f243e58972718f0a441a8
struct Run<'s> {
    /// What the hosted session is bound to.
    binding: &'s Binding,
    /// The request its compiler run was given.
    request: &'s Request,
}

/// The run the session is bound to, and an error naming the tool when the session is bound
/// to no compiler run. Nothing is read or written when there is none.
// @lfy def/mcp/main.lfy:task#task:compilerTool:09099b01d8e84ae85a89b2a12642ec68e8d2b277dfec75f117a5e093caf818b2
// @lfy def/mcp/main.lfy:dependency#dependency:compilerTool:94e97e4b9416897e8f7296ad1449f08bf7cb435fe9a76fc4125ba6de8559c49e
// @lfy def/mcp/main.lfy:mark#mark:compilerTool:56146f12eb0eb11e3ccbcf6b89b563f1173e5c188a183687178892cdd6aefb62
// @lfy def/mcp/main.lfy:submit#submit:compilerTool:ee05d8be309e4a7aab71c6ed469b76e49a958abace52e8b6407087bf66c21140
fn own_request<'s>(session: &'s Session, tool: &str) -> Result<Run<'s>, String> {
    match &session.binding {
        Some(binding) => match &binding.request {
            // The batch and the units acted on are the request's own, and the outputs are
            // read and written under the binding's directory, which is the root the hosted
            // session's workspace was read at.
            // @lfy def/mcp/main.lfy:task#task:compilerTool:1e78463366ba8fbbfbec5fc142e181f67f7b2aead81f243e58972718f0a441a8
            // @lfy def/mcp/main.lfy:task#task:compilerTool:a0f3f916620f9e289b39745855f3979f0a0db1a4ff2e30369ec791cd1d94087a
            // @lfy def/mcp/main.lfy:dependency#dependency:compilerTool:0044266dd6cf0877762ead31dc8ae4567556e4f98fae42343fc4f7d1616d887c
            // @lfy def/mcp/main.lfy:dependency#dependency:compilerTool:0c15908a4dcfda7bb04d0631866acd9f199cf990a0bd8a108b4e4d2744608e3a
            // @lfy def/mcp/main.lfy:mark#mark:compilerTool:a8577d54d63dba15d279a393a495d3cc11ecc51430e5ccdd9709b2c5dcb1d764
            // @lfy def/mcp/main.lfy:mark#mark:compilerTool:85e5595e052eeca50b56f9cebdba26de23ab6e384826458c5f80bff35a6a3579
            // @lfy def/mcp/main.lfy:submit#submit:compilerTool:60628d931d17c35840a5661d90ba57e6f4365679924ab922f3dc16beb99f6278
            // @lfy def/mcp/main.lfy:submit#submit:compilerTool:fe5e191c412762eabc28bb250d5cdf400a97c9315325f022cff7b603d32ee079
            Some(request) => Ok(Run { binding, request }),
            // @lfy def/mcp/main.lfy:dependency#dependency:compilerTool:94e97e4b9416897e8f7296ad1449f08bf7cb435fe9a76fc4125ba6de8559c49e
            None => Err(format!("{tool} acts only in a session bound to a compiler run")),
        },
        // @lfy def/mcp/main.lfy:mark#mark:compilerTool:56146f12eb0eb11e3ccbcf6b89b563f1173e5c188a183687178892cdd6aefb62
        // @lfy def/mcp/main.lfy:submit#submit:compilerTool:ee05d8be309e4a7aab71c6ed469b76e49a958abace52e8b6407087bf66c21140
        None => Err(format!("{tool} acts only in a session bound to a compiler run")),
    }
}

/// The units of a batch, as indices into the plan, leaving out any the plan does not hold.
// @lfy def/mcp/main.lfy:task#task:task:d056015a5c0120d12f5e1daaebd2c2f0c96947c0bbff29ed4ebdeb57af2bcbe2
fn batch_units(plan: &Plan, batch: &Batch) -> Vec<usize> {
    batch.units.iter().copied().filter(|&index| plan.units.get(index).is_some()).collect()
}

/// The stems of the units of a batch, joined.
// @lfy def/mcp/main.lfy:task#task:task:20cff63e998498e4042833f30de9a63a11338bcbcc0b15f2191ddcf2f7def4ab
fn batch_stems(plan: &Plan, batch: &Batch) -> String {
    let stems: Vec<&str> =
        batch_units(plan, batch).into_iter().map(|index| plan.units[index].stem.as_str()).collect();
    stems.join(", ")
}

/// A name with any leading definition file path and colon set aside: the path, when one was
/// spelled, and what remains.
// @lfy def/mcp/main.lfy:dependency#dependency:dependency:0e9dc230cab50f2d56b0c3bb4b3df7405447b216f7d728bbdb5c789f693619a0
// @lfy def/mcp/main.lfy:mark#mark:mark:ea0e414361181918c63cd2a060bf3bdae8d34bdc0cc992fa89cac7fc18138b2f
fn without_path(name: &str) -> (Option<&str>, &str) {
    match name.rsplit_once(':') {
        Some((path, rest)) if path.ends_with(".lfy") => (Some(path), rest),
        _ => (None, name),
    }
}

/// What remains of a name split into the identifier it begins with and the member name that
/// follows the first dot, when one does.
// @lfy def/mcp/main.lfy:dependency#dependency:dependency:0e9dc230cab50f2d56b0c3bb4b3df7405447b216f7d728bbdb5c789f693619a0
// @lfy def/mcp/main.lfy:mark#mark:mark:ea0e414361181918c63cd2a060bf3bdae8d34bdc0cc992fa89cac7fc18138b2f
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
// @lfy def/mcp/main.lfy:task#task:task:0ba2a96c6162e5b2088f0d0af27dcf6dcd7a99b009bf3be3772ffa13ffa864f4
// @lfy def/mcp/main.lfy:mark#mark:mark:1fe93b7a289817e552344c2be562a1942c5871b156c0bb103e88f3c5c41e4116
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
// @lfy def/mcp/main.lfy:task#task:task:0ba2a96c6162e5b2088f0d0af27dcf6dcd7a99b009bf3be3772ffa13ffa864f4
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
// @lfy def/mcp/main.lfy:task#task:task:48b5a17b6bd8bf11626b7e3c8a77f30620a9bcf76c647c3d0cb15aaa2ac3e582
fn origin_place(model: &Model, origin: NodeRef) -> String {
    let line = model.first_token(origin).map_or(1, |token| token.line);
    format!("{}:{line}", model.sources[origin.file].path)
}

/// One criterion or test of a request, read for a line of `elfie_task`.
// @lfy def/mcp/main.lfy:task#task:task:bc0cfc213f87dda7b8e1106191bb902bdb5a2b08b2472742020be295a621309b
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
// @lfy def/mcp/main.lfy:task#task:task:bc0cfc213f87dda7b8e1106191bb902bdb5a2b08b2472742020be295a621309b
// @lfy def/mcp/main.lfy:task#task:task:8bf5a91fa0df3686fb02ff5457cd19a9f3c4d034aceb88db292e7f850ecd6638
fn requirement_parts(model: &Model, requirement: &Requirement) -> Asked {
    match requirement {
        Requirement::Criterion(criterion) => Asked {
            id: criterion.id.clone(),
            entity: criterion.entity,
            place: origin_place(model, criterion.origin),
            text: criterion.text(),
        },
        // @lfy def/mcp/main.lfy:task#task:task:8bf5a91fa0df3686fb02ff5457cd19a9f3c4d034aceb88db292e7f850ecd6638
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
// @lfy def/mcp/main.lfy:task#task:task:8aab948f1ff4b43702f1158f16a22c2a5c3be0d41ed51e5ff90073af5c03d692
fn rewording_of<'r>(request: &'r Request, path: &str, id: &str) -> Option<&'r Rewording> {
    request.reworded.get(path)?.iter().find(|rewording| rewording.id == id)
}

/// What a criterion or test of a unit is marked as: `reworded` when its id rewords one the
/// unit's existing outputs were generated against, `kept` when those outputs were generated
/// against it, `new` when they were generated against a known set that does not hold it or
/// there are no outputs yet, and `unknown` when what they answered is not recorded.
// @lfy def/mcp/main.lfy:task#task:task:00479e8dc9cc7003d7a8fc71ee72b37424f99ac82adaf9497bd8a64df5cd83a2
// @lfy def/mcp/main.lfy:task#task:task:aa3aedc17d31c8c2288f74ab8d995d37aabd29e988ed6623b3ab368c90880080
// @lfy def/mcp/main.lfy:task#task:task:3fd2e90fbb5affd82dc46bf4e87ba7bcb8ba2f4df7047de83fea316774ee4d01
fn mark_of(request: &Request, path: &str, has_outputs: bool, id: &str) -> &'static str {
    // A criterion that kept its place and changed its words took a new id, which the compile
    // has already moved its markers to, so it is marked apart from what is really new.
    // @lfy def/mcp/main.lfy:task#task:task:8aab948f1ff4b43702f1158f16a22c2a5c3be0d41ed51e5ff90073af5c03d692
    if rewording_of(request, path, id).is_some() {
        return "reworded";
    }
    match request.answered.get(path) {
        // @lfy def/mcp/main.lfy:task#task:task:00479e8dc9cc7003d7a8fc71ee72b37424f99ac82adaf9497bd8a64df5cd83a2
        Some(answered) if answered.iter().any(|known| known == id) => "kept",
        // @lfy def/mcp/main.lfy:task#task:task:aa3aedc17d31c8c2288f74ab8d995d37aabd29e988ed6623b3ab368c90880080
        Some(_) => "new",
        None if !has_outputs => "new",
        // @lfy def/mcp/main.lfy:task#task:task:3fd2e90fbb5affd82dc46bf4e87ba7bcb8ba2f4df7047de83fea316774ee4d01
        None => "unknown",
    }
}

/// What this compiler run must do, for each unit of its batch or for the one named.
// @lfy def/mcp/main.lfy:task#task:task:03c0e861c4bbd25a35c249a398a25887a28c2cef4fe08f743b831393be179644
pub fn task(session: &Session, unit: Option<&str>) -> Result<String, String> {
    let run = own_request(session, "elfie_task")?;
    let (binding, request) = (run.binding, run.request);
    // Everything below is read from the request and from the program the compile lowered;
    // no definition file and no output is read.
    // @lfy def/mcp/main.lfy:task#task:task:03c0e861c4bbd25a35c249a398a25887a28c2cef4fe08f743b831393be179644
    let plan = &binding.plan;
    let program = &binding.program;
    let workspace = &program.workspace;
    let model = &workspace.model;
    let batch = &request.batch;
    // @lfy def/mcp/main.lfy:task#task:task:d056015a5c0120d12f5e1daaebd2c2f0c96947c0bbff29ed4ebdeb57af2bcbe2
    let indexes: Vec<usize> = match unit {
        Some(stem) => {
            let found: Vec<usize> = batch_units(plan, batch)
                .into_iter()
                .filter(|&index| plan.units[index].stem == stem)
                .collect();
            if found.is_empty() {
                // @lfy def/mcp/main.lfy:task#task:task:20cff63e998498e4042833f30de9a63a11338bcbcc0b15f2191ddcf2f7def4ab
                return Err(format!(
                    "no unit of this batch has the stem {stem}; the units of {} are: {}",
                    batch.identifier,
                    batch_stems(plan, batch)
                ));
            }
            found
        }
        None => batch_units(plan, batch),
    };
    let mut lines: Vec<String> = Vec::new();
    let (mut fresh, mut rephrased, mut kept, mut unknown, mut removed) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    for index in indexes {
        let of_unit = &plan.units[index];
        let file = &workspace.files[of_unit.file];
        let target = &workspace.targets[of_unit.target];
        let reason = of_unit.reason.map_or_else(|| "up to date".to_string(), |reason| reason.as_str().to_string());
        // @lfy def/mcp/main.lfy:task#task:task:260c99a22d5afd2cb5bfe5e5341310aff3e4b5c20a62493303d03bc95d6f4767
        lines.push(format!("unit {} {} {reason}", of_unit.stem, file.path));
        if of_unit.outputs.is_empty() {
            // @lfy def/mcp/main.lfy:task#task:task:3701f0af0023dfc1eadd1dee395415c17f3523d19c39a1d6d8f30a8aff532e9e
            lines.push(format!(
                "no output yet; its outputs go under {} at the paths the guidance spells from the stem {}",
                target.output_directory, of_unit.stem
            ));
        } else {
            // @lfy def/mcp/main.lfy:task#task:task:93d986fe4163c82020f47668d2a385e3662ee162cec7a9332137b5df9c98b068
            let outputs: Vec<&str> = of_unit.outputs.iter().map(|map| map.output.as_str()).collect();
            lines.push(format!("outputs {}", outputs.join(", ")));
        }
        let named = named_in_unit(model, workspace, of_unit);
        let own = workspace.files[of_unit.file].source;
        let requirements: &[Requirement] =
            request.requirements.get(&file.path).map(Vec::as_slice).unwrap_or(&[]);
        let mut headed: Vec<EntityId> = Vec::new();
        let mut answered: Vec<EntityId> = Vec::new();
        // @lfy def/mcp/main.lfy:task#task:task:e6bdc2cd8bf73910039781fad6be14d73994cd9495ecfb29415132f0e0b7926d
        for requirement in requirements {
            let Asked { id, entity, place, text } = requirement_parts(model, requirement);
            if let Some(entity) = entity {
                if !answered.contains(&entity) {
                    answered.push(entity);
                }
                // @lfy def/mcp/main.lfy:task#task:task:0ba2a96c6162e5b2088f0d0af27dcf6dcd7a99b009bf3be3772ffa13ffa864f4
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
                    // An entity moved into the unit is declared in another file, which the
                    // line names so that a marker for it spells that file.
                    // @lfy def/mcp/main.lfy:task#task:task:a151226a2f7853ca59f5d23754f07954956c81793b8a0b4e771b88a500617003
                    match model.entities[entity].file.filter(|&source| source != own) {
                        Some(source) => {
                            lines.push(format!("{name} declared in {}", model.sources[source].path));
                        }
                        None => lines.push(name),
                    }
                }
            }
            let marked = mark_of(request, &file.path, !of_unit.outputs.is_empty(), &id);
            match marked {
                "kept" => kept += 1,
                "unknown" => unknown += 1,
                "reworded" => rephrased += 1,
                _ => fresh += 1,
            }
            // @lfy def/mcp/main.lfy:task#task:task:bc0cfc213f87dda7b8e1106191bb902bdb5a2b08b2472742020be295a621309b
            lines.push(format!("{marked} {id} {place} — {text}"));
            // What a reworded one replaces, on the line after it.
            // @lfy def/mcp/main.lfy:task#task:task:8aab948f1ff4b43702f1158f16a22c2a5c3be0d41ed51e5ff90073af5c03d692
            if let Some(rewording) = rewording_of(request, &file.path, &id) {
                lines.push(format!("replaces {} — {}", rewording.replaces, rewording.text));
            }
        }
        // Every entity of the unit must carry a marker, so those with nothing to answer for
        // are named: no criterion and no test of the unit is for them.
        // @lfy def/mcp/main.lfy:task#task:task:f9fb73337a13788bc2c97891ea0b6725150d8f585b1ea3c8102578a43fdc42d6
        let without: Vec<String> = of_unit
            .entities
            .iter()
            .filter(|entity| !answered.contains(entity))
            .map(|&entity| identifier_of(model, entity))
            .collect();
        if without.is_empty() {
            // @lfy def/mcp/main.lfy:task#task:task:8cda0fab4d2e20c30f36514faa7a119d7713f6defa2830d8a2284e0161739f87
            lines.push("no entity of the unit lacks a criterion or test".to_string());
        } else {
            // @lfy def/mcp/main.lfy:task#task:task:f9fb73337a13788bc2c97891ea0b6725150d8f585b1ea3c8102578a43fdc42d6
            lines.push(format!(
                "no criterion or test is for {}; each still needs a marker",
                without.join(", ")
            ));
        }
        match request.removed.get(&file.path) {
            // @lfy def/mcp/main.lfy:task#task:task:0e1730d59363044ef74bcf4673aef8eae5aecf93a017aa90bc7f8d9d457f6afd
            Some(gone) if !gone.is_empty() => {
                lines.push("these were removed since its outputs were accepted".to_string());
                for one in gone {
                    removed += 1;
                    // A removed criterion or test the request could not quote carries no
                    // text, and no place either, since the revision that held it is not the
                    // one its place would be read from; the line says so rather than
                    // quoting nothing.
                    // @lfy def/mcp/main.lfy:task#task:task:de5f7c473194374bfa34413eb23292d2536ef397d3077ff1897532c68ab7052d
                    if one.text.is_empty() {
                        lines.push(format!("removed {} — its text is not known", one.id));
                        continue;
                    }
                    // @lfy def/mcp/main.lfy:task#task:task:0e1730d59363044ef74bcf4673aef8eae5aecf93a017aa90bc7f8d9d457f6afd
                    lines.push(format!("removed {} {} — {}", one.id, one.place, one.text));
                }
            }
            // @lfy def/mcp/main.lfy:task#task:task:c6544cb71fb79033882e38ad1d044989c8fa3a6dc97e274b3d4054fbb9b65bda
            None if !of_unit.outputs.is_empty() => {
                lines.push("which criteria and tests were removed is not known".to_string());
            }
            _ => {}
        }
    }
    // @lfy def/mcp/main.lfy:task#task:task:ca1abaa819942eb8c6c130e520f1c36ee16815c41b54d388cca6a9b8ad14a76c
    lines.push(format!(
        "{fresh} new, {rephrased} reworded, {kept} kept, {unknown} unknown, {removed} removed"
    ));
    Ok(lines.join("\n"))
}

/// The lines of one entity in the interface text of a unit: its own line, then the line of
/// each member or enum member it declares, or only the line of one member when one is named.
// @lfy def/mcp/main.lfy:dependency#dependency:dependency:5187f431c8e1a018407dfaf23c821f2cfc1f4a15370dc6acb8175a28e7fcd50c
// @lfy def/mcp/main.lfy:dependency#dependency:dependency:82fa16bd07081751c96e7033b5801a1698a74ccf133725c2d1e01ea9edd01680
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
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:82fa16bd07081751c96e7033b5801a1698a74ccf133725c2d1e01ea9edd01680
        match member {
            Some(member) if !rest.starts_with(&format!("- `{member}` (")) => {}
            _ => found.push(line.to_string()),
        }
    }
    found
}

/// What one entity of a dependency of this compiler run's batch offers its dependents.
// @lfy def/mcp/main.lfy:dependency#dependency:dependency:88edd00efeadac6571b81f940a3b239335ff17aac9c416776584049ade3de935
pub fn dependency(session: &Session, name: &str) -> Result<String, String> {
    let run = own_request(session, "elfie_dependency")?;
    let (binding, request) = (run.binding, run.request);
    // The interfaces are the request's own, and the lines come from the program the compile
    // lowered; no definition file and no output is read.
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:88edd00efeadac6571b81f940a3b239335ff17aac9c416776584049ade3de935
    let plan = &binding.plan;
    let workspace = &binding.program.workspace;
    let model = &workspace.model;
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:0e9dc230cab50f2d56b0c3bb4b3df7405447b216f7d728bbdb5c789f693619a0
    let (in_file, rest) = without_path(name);
    let (identifier, member) = identifier_and_member(rest);
    let mut sections: Vec<String> = Vec::new();
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:48fa7258825a24866309481b9e6048acc49c37a1e74ed155a56a3210a884e862
    for interface in &request.interfaces {
        let Some(of_unit) = plan.units.get(interface.unit) else { continue };
        let file = &workspace.files[of_unit.file];
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:7b425db08ad081030e6137f0d6924c40b3b223d36d698551284b593335193519
        if in_file.is_some_and(|path| path != file.path) {
            continue;
        }
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:48fa7258825a24866309481b9e6048acc49c37a1e74ed155a56a3210a884e862
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
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:53420221788d685744cc63a541e15f79755a4b23efe12ec64289b0e9c90ad313
        let mut out = format!("{} {}\noutputs {}", of_unit.stem, file.path, interface.outputs.join(", "));
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:5187f431c8e1a018407dfaf23c821f2cfc1f4a15370dc6acb8175a28e7fcd50c
        for line in found {
            out.push('\n');
            out.push_str(&line);
        }
        sections.push(out);
    }
    if sections.is_empty() {
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:57ea76cedd14a64116b4e927fb4f9f13577e2df3fe6caa9fd1c92a2dd6d88d06
        return Err(format!(
            "no dependency outside the batch declares {name}; elfie_entity reads an entity of the batch itself, or of any file"
        ));
    }
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:52155c3bef2ef807556011be184c10507617e56c46ebfded60f2e934d1a72699
    Ok(sections.join("\n\n---\n\n"))
}

// ---- the markers of a compiler run --------------------------------------------------

// Decision: an id is 64 hex digits and more, which an agent copies wrong often enough to
// matter, so the server writes a marker's id from one it has checked, and moves every marker
// of a reworded criterion to its new id in one call.

/// The source paths a marker of one of this batch's regions may name: the file of each unit
/// of the batch, and the file of each entity moved into one.
// @lfy def/mcp/main.lfy:mark#mark:mark:aac83695fcc88a2c0a4688d77974f2d0a6805ebf6b2d752b2141ff71a8a745a2
fn own_sources(run: &Run<'_>) -> Vec<String> {
    let binding = run.binding;
    let workspace = &binding.program.workspace;
    let model = &workspace.model;
    let mut found: Vec<String> = Vec::new();
    for index in batch_units(&binding.plan, &run.request.batch) {
        let of_unit = &binding.plan.units[index];
        let mut paths = vec![workspace.files[of_unit.file].path.clone()];
        for &entity in &of_unit.entities {
            if let Some(source) = model.entities[entity].file {
                paths.push(model.sources[source].path.clone());
            }
        }
        for path in paths {
            if !found.contains(&path) {
                found.push(path);
            }
        }
    }
    found
}

/// The target of the batch of a compiler run, read off the session's workspace so that its
/// output directory is read under the binding's directory; `None` when the batch is empty.
// @lfy def/mcp/main.lfy:mark#mark:mark:20e94fd8b37ced24d2351848435ef39376d3fc15eeb5d4e64ccffd5f95cb4053
fn batch_own_target<'w>(workspace: &'w Workspace, run: &Run<'_>) -> Option<&'w Target> {
    let index = *run.request.batch.units.first()?;
    let of_unit = run.binding.plan.units.get(index)?;
    workspace.targets.get(of_unit.target)
}

/// One criterion or test of the batch's units, with the unit it belongs to, by id.
// @lfy def/mcp/main.lfy:mark#mark:mark:165048edf3b25a24ddb15a6916231fdc341c3ce643d5b2d74c220ae0c65ab193
fn requirement_unit(run: &Run<'_>, id: &str) -> Option<usize> {
    let binding = run.binding;
    let workspace = &binding.program.workspace;
    batch_units(&binding.plan, &run.request.batch).into_iter().find(|&index| {
        let path = &workspace.files[binding.plan.units[index].file].path;
        run.request
            .requirements
            .get(path)
            .is_some_and(|requirements| requirements.iter().any(|one| one.id() == id))
    })
}

/// Whether one id is that of a global criterion or test of the request.
// @lfy def/mcp/main.lfy:mark#mark:mark:dd3ef6b416a23f341639e29ff2f36569d059617f0c4d6c5404dea75abb9f0ef2
fn is_global_requirement(request: &Request, id: &str) -> bool {
    request.globals.iter().any(|one| one.id() == id)
}

/// Rewrite to a new id every marker of this run's outputs that names an old one.
///
/// Which markers those are is [`generation::rename`]'s to decide, and it decides it as
/// acceptance does: only the markers naming the file of the unit the new id belongs to, or
/// of a file declaring an entity moved into it, so another unit's markers are never touched.
/// The outputs it returns are the ones whose markers it rewrote, and only those are written.
// @lfy def/mcp/main.lfy:mark#mark:mark:4f60b16548a3a5d470c0ee9cd9a0c70970ffdedc84b94fa5e4f5efbde3465999
fn rename_markers(
    session: &Session,
    run: &Run<'_>,
    target: &Target,
    id: &str,
    replaces: &str,
) -> Result<String, String> {
    let workspace = &session.workspace;
    let binding = run.binding;
    // @lfy def/mcp/main.lfy:mark#mark:mark:165048edf3b25a24ddb15a6916231fdc341c3ce643d5b2d74c220ae0c65ab193
    let Some(index) = requirement_unit(run, id) else {
        return Err(format!(
            "{id} is no criterion or test of the units of the batch {}; elfie_task lists them",
            run.request.batch.identifier
        ));
    };
    // The output files of the batch's target as they are when the call arrives.
    // @lfy def/mcp/main.lfy:mark#mark:mark:9cbf7b9aff22e535179edfb2494ceb9e2aa081cb354077d9a0d66e38056c48d6
    let mut read: Vec<Output> = Vec::new();
    for output in output_files(workspace, target) {
        let path = workspace.root.join(&output);
        let Ok(text) = fs::read_to_string(&path) else { continue };
        read.push(Output { path: output, text });
    }
    let ids = BTreeMap::from([(replaces.to_string(), id.to_string())]);
    // @lfy def/mcp/main.lfy:mark#mark:mark:9cbf7b9aff22e535179edfb2494ceb9e2aa081cb354077d9a0d66e38056c48d6
    let renamed = generation::rename(workspace, &binding.plan, index, &read, &ids);
    let mut rewritten: Vec<String> = Vec::new();
    for output in &renamed {
        // @lfy def/mcp/main.lfy:mark#mark:mark:9cbf7b9aff22e535179edfb2494ceb9e2aa081cb354077d9a0d66e38056c48d6
        let path = workspace.root.join(&output.path);
        fs::write(&path, &output.text)
            .map_err(|error| format!("{} cannot be written: {error}", output.path))?;
        // The lines that differ from the file as it was read are the rewritten markers, since
        // `rename` keeps every other line of an output it returns.
        // @lfy def/mcp/main.lfy:mark#mark:mark:4f60b16548a3a5d470c0ee9cd9a0c70970ffdedc84b94fa5e4f5efbde3465999
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
            // @lfy def/mcp/main.lfy:mark#mark:mark:5bb1f1250f7f259cd65c04fd3be30c251cc135967710c7ce195a78f31866f8fe
            for marker in markers_on(&after, at + 1) {
                rewritten.push(format!("{}:{} {}", output.path, at + 1, marker_text(marker)));
            }
        }
    }
    if rewritten.is_empty() {
        // @lfy def/mcp/main.lfy:mark#mark:mark:675bf462c93e343e85219da2087f1672279fc0bc713a21b5d0f5550e274a6a40
        return Err(format!(
            "no marker of the output files of the target {} naming {} names {replaces}",
            target.identifier,
            workspace.files[binding.plan.units[index].file].path
        ));
    }
    // @lfy def/mcp/main.lfy:mark#mark:mark:5bb1f1250f7f259cd65c04fd3be30c251cc135967710c7ce195a78f31866f8fe
    let mut text = format!("{} marker{} rewritten", rewritten.len(), if rewritten.len() == 1 { "" } else { "s" });
    for line in rewritten {
        text.push('\n');
        text.push_str(&line);
    }
    Ok(text)
}

/// The entity one name picks out of the batch's units, with the unit it belongs to and the
/// path of the file that declares it.
// @lfy def/mcp/main.lfy:mark#mark:mark:1fe93b7a289817e552344c2be562a1942c5871b156c0bb103e88f3c5c41e4116
fn entity_named(run: &Run<'_>, entity: &str) -> Result<(usize, String, String), String> {
    let (binding, request) = (run.binding, run.request);
    let workspace = &binding.program.workspace;
    let model = &workspace.model;
    // @lfy def/mcp/main.lfy:mark#mark:mark:ea0e414361181918c63cd2a060bf3bdae8d34bdc0cc992fa89cac7fc18138b2f
    let (in_file, rest) = without_path(entity);
    let (identifier, member) = identifier_and_member(rest);
    let mut found: Vec<(usize, String, String)> = Vec::new();
    for index in batch_units(&binding.plan, &request.batch) {
        for &candidate in &binding.plan.units[index].entities {
            if model.entities[candidate].identifier.as_deref() != Some(identifier) {
                continue;
            }
            let Some(source) = model.entities[candidate].file else { continue };
            let path = model.sources[source].path.clone();
            if in_file.is_some_and(|named| named != path) {
                continue;
            }
            // @lfy def/mcp/main.lfy:mark#mark:mark:1fe93b7a289817e552344c2be562a1942c5871b156c0bb103e88f3c5c41e4116
            if let Some(member) = member
                && !model
                    .members(candidate)
                    .into_iter()
                    .any(|symbol| model.symbols[symbol].name == member)
            {
                continue;
            }
            let name = match member {
                Some(member) => format!("{identifier}.{member}"),
                None => identifier.to_string(),
            };
            if !found.iter().any(|(_, _, spelled)| spelled == &path) {
                found.push((index, name, path));
            }
        }
    }
    match found.len() {
        // @lfy def/mcp/main.lfy:mark#mark:mark:64c1518801f24801feb6b601cbda298cc2a1a97c81889b15a61fe32d5a3ded45
        0 => Err(format!(
            "no unit of the batch {} declares {entity}; its units are: {}",
            request.batch.identifier,
            batch_stems(&binding.plan, &request.batch)
        )),
        1 => Ok(found.remove(0)),
        // @lfy def/mcp/main.lfy:mark#mark:mark:a38062a1fcda5efa808baa27f8d58d69ab0670d6640e713cece4075635abc240
        _ => {
            let files: Vec<&str> = found.iter().map(|(_, _, path)| path.as_str()).collect();
            Err(format!(
                "{entity} names entities of more than one file ({}); a file path and a colon before the name chooses one",
                files.join(", ")
            ))
        }
    }
}

/// Write one marker line into one of this run's outputs.
// @lfy def/mcp/main.lfy:mark#mark:mark:e959da5d59df47ac7933bb07c057234854cf20b78a1ae1296c4d2521057c5f32
fn insert_marker(
    session: &Session,
    run: &Run<'_>,
    target: &Target,
    id: Option<&str>,
    file: &str,
    line: usize,
    entity: &str,
) -> Result<String, String> {
    let workspace = &session.workspace;
    let binding = run.binding;
    // @lfy def/mcp/main.lfy:mark#mark:mark:62c0bc17d8c1d1e37e6821582cd6b59f05e0204a3f3edd6f1e33e9d2597fc2db
    if !output_files(workspace, target).iter().any(|known| known == file) {
        return Err(format!(
            "{file} is not an output file of the target {}; write the output first, then insert a marker into it",
            target.identifier
        ));
    }
    let path = workspace.root.join(file);
    let text = fs::read_to_string(&path).map_err(|error| format!("{file} cannot be read: {error}"))?;
    let count = text.lines().count();
    // @lfy def/mcp/main.lfy:mark#mark:mark:a86c8a8dd444a7e09d183e1c131c3e4e0e74fdc0a4a752698290645da729a4b0
    if line < 1 || line > count + 1 {
        return Err(format!("{file} has {count} lines, so line must be between 1 and {}", count + 1));
    }
    // The last marker before the line says whose region the line falls in; a region of
    // another unit is never written into.
    // @lfy def/mcp/main.lfy:mark#mark:mark:aac83695fcc88a2c0a4688d77974f2d0a6805ebf6b2d752b2141ff71a8a745a2
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
            .map(|of_unit| format!(" the unit {}", of_unit.stem))
            .unwrap_or_default();
        return Err(format!(
            "line {line} of {file} falls in a region of {}, which belongs to{}",
            before.file,
            if owner.is_empty() { " no unit of this batch".to_string() } else { owner }
        ));
    }
    let (index, name, declaring) = entity_named(run, entity)?;
    // @lfy def/mcp/main.lfy:mark#mark:mark:dd3ef6b416a23f341639e29ff2f36569d059617f0c4d6c5404dea75abb9f0ef2
    if let Some(id) = id {
        let of_unit = &binding.plan.units[index];
        let source = &binding.program.workspace.files[of_unit.file].path;
        let local = run
            .request
            .requirements
            .get(source)
            .is_some_and(|requirements| requirements.iter().any(|one| one.id() == id));
        if !local && !is_global_requirement(run.request, id) {
            return Err(format!(
                "{id} is no criterion or test of the unit {} and no global one; elfie_task lists them",
                of_unit.stem
            ));
        }
    }
    let mut pieces: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
    // The line break is the one the file's first line ends with, and a line feed when it has
    // none.
    // @lfy def/mcp/main.lfy:mark#mark:mark:f81ece6f1d736c69189687f1b61d0a5d106ce51765776bbd8523caaecef41dea
    let ending = match pieces.first() {
        Some(first) if first.ends_with("\r\n") => "\r\n",
        _ => "\n",
    };
    // @lfy def/mcp/main.lfy:mark#mark:mark:e959da5d59df47ac7933bb07c057234854cf20b78a1ae1296c4d2521057c5f32
    let indent: String = match pieces.get(line.saturating_sub(1)).filter(|_| line <= count) {
        Some(at) => at.chars().take_while(|&character| character == ' ' || character == '\t').collect(),
        None => String::new(),
    };
    let inserted = match id {
        Some(id) => format!("{indent}{} @lfy {declaring}:{name}#{id}", target.marker_comment),
        None => format!("{indent}{} @lfy {declaring}:{name}", target.marker_comment),
    };
    if line > count {
        // @lfy def/mcp/main.lfy:mark#mark:mark:0d151705612442f586374d3ecc64db058c44120fe7a7355be4a84f2aafe6d259
        if let Some(last) = pieces.last_mut()
            && !last.ends_with('\n')
        {
            last.push_str(ending);
        }
        pieces.push(format!("{inserted}{ending}"));
    } else {
        // @lfy def/mcp/main.lfy:mark#mark:mark:f81ece6f1d736c69189687f1b61d0a5d106ce51765776bbd8523caaecef41dea
        pieces.insert(line - 1, format!("{inserted}{ending}"));
    }
    fs::write(&path, pieces.concat()).map_err(|error| format!("{file} cannot be written: {error}"))?;
    let at = line.min(count + 1);
    // @lfy def/mcp/main.lfy:mark#mark:mark:2e402e1e9ad7b4d3ccb7a990b41387cfa8be2150d5feaa6d160ee8adfd91704e
    Ok(format!("{file}:{at} {inserted}"))
}

/// Write a marker line into one of this compiler run's outputs, or rewrite to a new id every
/// marker of its outputs that names an old one, so that no id is typed by hand.
///
/// Nothing but an output file of the batch's target is written: no source map and no review
/// record, which the CLI writes, and no file outside that target's output directory.
// @lfy def/mcp/main.lfy:mark#mark:mark:a7dcf7eb424415c0fcc0ed5a98c1d56456a130be8cb8f194d3f21ec67c1606df
// @lfy def/mcp/main.lfy:mark#mark:mark:fcfecab8991108e6f27911bb4b6d77cfffcf93ddbb785581af7a1021cbe3b53c
pub fn mark(
    session: &Session,
    id: Option<&str>,
    replaces: Option<&str>,
    file: Option<&str>,
    line: Option<usize>,
    entity: Option<&str>,
) -> Result<String, String> {
    let run = own_request(session, "elfie_mark")?;
    let Some(target) = batch_own_target(&session.workspace, &run) else {
        return Err(format!("the batch {} holds no unit", run.request.batch.identifier));
    };
    // The arguments a call gives say what it wants: `replaces` asks for a rename, and `file`,
    // `line`, and `entity` ask for an insertion.
    // @lfy def/mcp/main.lfy:mark#mark:mark:de06da3d9e51338cb41d6032e36580ce6510b4297822e054fb6f234dc68b0594
    if let Some(replaces) = replaces {
        if id.is_none() || file.is_some() || line.is_some() || entity.is_some() {
            return Err(
                "a rename takes replaces and id and nothing else; leave out file, line, and entity".to_string()
            );
        }
        let id = id.unwrap_or_default();
        return rename_markers(session, &run, target, id, replaces);
    }
    // @lfy def/mcp/main.lfy:mark#mark:mark:2959e7495b60ed84e030a118eff9b9a1cb5d699815713975e3e7f53137050e14
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
// @lfy def/mcp/main.lfy:submit#submit:submit:f94daa1287cb9c2fd43bbdfe11f8cfe51bf9706696102840706f0445f387669a
const ENDS_WITH: &str = "ELFIE: DONE, ELFIE: BLOCKED: and the reason, or ELFIE: CLARIFY: and the question";

/// Say how this compiler run ended. Nothing is written: the outcome is kept in the session
/// until the CLI closes it.
// @lfy def/mcp/main.lfy:submit#submit:submit:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
pub fn submit(session: &Session, outcome: &str, message: &str) -> Result<String, String> {
    own_request(session, "elfie_submit")?;
    // @lfy def/mcp/main.lfy:submit#submit:submit:28e49a28fbbfa8f85d4d95ecccb6f05ae599e3a6f128b655d73257666a31412e
    let Some(kind) = SubmittedOutcome::from_name(&outcome.to_ascii_lowercase()) else {
        return Err(format!(
            "the outcome {outcome} is none of {}, {}, and {}",
            SubmittedOutcome::Done.as_str(),
            SubmittedOutcome::Blocked.as_str(),
            SubmittedOutcome::Clarify.as_str()
        ));
    };
    // @lfy def/mcp/main.lfy:submit#submit:submit:b99c292c6a953720125d01484c7cc9fc03b9727847086a54737c3363884273cb
    if matches!(kind, SubmittedOutcome::Blocked | SubmittedOutcome::Clarify) && message.trim().is_empty() {
        return Err(format!(
            "{} needs its reason and {} its question",
            SubmittedOutcome::Blocked.as_str(),
            SubmittedOutcome::Clarify.as_str()
        ));
    }
    // @lfy def/mcp/main.lfy:submit#submit:submit:839e3605860269cb5d2a142eae47825edd5f8e18cc86e1a3b9c4f70278331d8d
    let mut recorded = session.submission.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *recorded = Some(Submission {
        outcome: kind,
        message: message.to_string(),
    });
    // @lfy def/mcp/main.lfy:submit#submit:submit:bedeb2634345b2348c1509d268083160d2e9755e210b5107f4fbc972522749c1
    // @lfy def/mcp/main.lfy:submit#submit:submit:f94daa1287cb9c2fd43bbdfe11f8cfe51bf9706696102840706f0445f387669a
    Ok(format!(
        "recorded the outcome {}; a later call of elfie_submit replaces it\nend your final message with the one line for the outcome: {ENDS_WITH}",
        kind.as_str()
    ))
}
