// @lfy def/mcp/main.lfy:output
//! The tools that read the generated code: where the regions of an entity or of a
//! requirement are, where one line came from, and which lines match a pattern.

use super::*;

// Decision: the source maps these tools read are `generation::source_maps_of` the
// workspace, as elfie_units reads them, so what a tool answers is what the last acceptance
// recorded in elfie-compile/maps, never a guess from the text on disk.

/// The language a fenced block of an output is marked with: the output's extension, or
/// nothing when it has none.
// @lfy def/mcp/main.lfy:output#output:output:b5c9012118ce16c51f508f0dc7a730f69985ec6d3e5351b99d447dc69d898590
fn fence_of(path: &str) -> &str {
    Path::new(path).extension().and_then(|extension| extension.to_str()).unwrap_or("")
}

/// The lines of an output from `first` through `last`, counting from 1, as they are on
/// disk; the empty text when the file cannot be read.
// @lfy def/mcp/main.lfy:output#output:output:b5c9012118ce16c51f508f0dc7a730f69985ec6d3e5351b99d447dc69d898590
fn excerpt(workspace: &Workspace, output: &str, first: usize, last: usize) -> String {
    let Ok(text) = fs::read_to_string(workspace.root.join(output)) else {
        return String::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    let from = first.saturating_sub(1).min(lines.len());
    let to = last.min(lines.len()).max(from);
    lines[from..to].join("\n")
}

/// One region of an output: its path, a colon, the line it begins on, a dash, the line it
/// ends on, a space, and what it came from, then, when `text` is true, its text in a fenced
/// block; with `text` false the line stands alone, so however long an output is its regions
/// read as a few lines.
// @lfy def/mcp/main.lfy:output#output:output:22ddcfeba7456aa91005e6aeab2fcd871e9c512ede2849961438a5b7a3cc5d7c
fn region_block(workspace: &Workspace, output: &str, marker: &Marker, text: bool) -> String {
    let mut heading = format!("{}:{}-{}", output, marker.output_line, marker.end);
    if let Some(entity) = &marker.entity {
        heading.push_str(&format!(" {entity}"));
    }
    // The marker names a requirement, so the line ends with its id, after the entity.
    // @lfy def/mcp/main.lfy:output#output:output:f06f38040e8ba03b92a99e321da9419cc77651260ecc24f8c0087ce2ba8f1dcd
    if let Some(requirement) = &marker.requirement {
        heading.push_str(&format!(" {requirement}"));
    }
    if !text {
        // @lfy def/mcp/main.lfy:output#output:output:a388e41750cae72a1a126c53b636547055414dad9404ae10cd679aed3861fe6a
        return heading;
    }
    // @lfy def/mcp/main.lfy:output#output:output:b5c9012118ce16c51f508f0dc7a730f69985ec6d3e5351b99d447dc69d898590
    format!(
        "{heading}\n```{}\n{}\n```",
        fence_of(output),
        excerpt(workspace, output, marker.output_line, marker.end)
    )
}

/// Every region of every source map, each with the output that holds it, in source map then
/// output order, limited to one target when one is given.
// @lfy def/mcp/main.lfy:output#output:output:18bbd05acd9d7ef32f170d4713899aaf2e4c19f93da25c7feae2938e50414fc1
fn every_region<'m>(maps: &'m [SourceMap], target: Option<&str>) -> Vec<(&'m str, &'m Marker)> {
    maps.iter()
        .filter(|map| !target.is_some_and(|target| map.target != target))
        .flat_map(|map| map.markers.iter().map(|marker| (map.output.as_str(), marker)))
        .collect()
}

/// The directories a build or a version control system keeps for itself, which hold no
/// output however deep below a target's output directory they lie.
// @lfy def/mcp/main.lfy:output#output:output:1d6736275de07ac86ca7ade748c7890f146871f150c5b312ffe1d56e39b23e05
const KEPT_DIRECTORIES: [&str; 3] = ["target", ".git", "node_modules"];

/// Whether a path, relative to the workspace root, lies below one of those directories.
/// Only the segments below the target's output directory are read, and the file's own name
/// is no segment of them, so a target that writes under a directory of that name is read as
/// it is written.
// @lfy def/mcp/main.lfy:output#output:output:1d6736275de07ac86ca7ade748c7890f146871f150c5b312ffe1d56e39b23e05
fn below_a_kept_directory(path: &str, output_directory: &str) -> bool {
    let under = path.strip_prefix(output_directory).unwrap_or(path);
    let under = under.strip_prefix('/').unwrap_or(under);
    let mut segments: Vec<&str> = under.split('/').collect();
    segments.pop();
    segments.iter().any(|segment| KEPT_DIRECTORIES.contains(segment))
}

/// Whether a path, relative to the workspace root, is an output file of a target of the
/// session: under the target's output directory, with an extension among the target's, and
/// there on disk, but below none of the directories a build or a version control system
/// keeps. Limited to one target when one is named.
// @lfy def/mcp/main.lfy:output#output:output:e3ada846896a5d797c74467acb25cc6ab9c9f3667f13eae813f8169d01373c8f
fn is_output_file(workspace: &Workspace, path: &str, target: Option<&str>) -> bool {
    workspace
        .targets
        .iter()
        .filter(|of_target| target.is_none_or(|target| of_target.identifier == target))
        .any(|of_target| {
            // @lfy def/mcp/main.lfy:output#output:output:1d6736275de07ac86ca7ade748c7890f146871f150c5b312ffe1d56e39b23e05
            !below_a_kept_directory(path, &of_target.output_directory)
                && output_files(workspace, of_target).iter().any(|known| known == path)
        })
}

/// Every marker one output file holds as it is read now, as acceptance reads them: the ends
/// derived over every marker of the file, then the markers naming a file that is not in the
/// program left out, since nothing was generated from a file the program does not hold.
// @lfy def/mcp/main.lfy:output#output:output:5f91a9d6a55f8212f9288be65dd547c69391009665dc988e76b8a5150ac20406
// @lfy def/mcp/main.lfy:output#output:output:84421a06b6eda0b985d11eb52870e3f12014a1594070085b9dc237f580115624
fn markers_now(workspace: &Workspace, path: &str) -> Vec<Marker> {
    let Ok(text) = fs::read_to_string(workspace.root.join(path)) else {
        return Vec::new();
    };
    generation::parse_markers(&text)
        .into_iter()
        // @lfy def/mcp/main.lfy:output#output:output:84421a06b6eda0b985d11eb52870e3f12014a1594070085b9dc237f580115624
        .filter(|marker| workspace.file(&marker.file).is_some())
        .collect()
}

/// Where generated code is, with its text or without it: every region that came from one
/// entity, that answers for one criterion or test, or that one output file holds.
// @lfy def/mcp/main.lfy:output#output:output:18bbd05acd9d7ef32f170d4713899aaf2e4c19f93da25c7feae2938e50414fc1
pub fn output(session: &Session, name: &str, target: Option<&str>, text: bool) -> Result<String, String> {
    let workspace = &session.workspace;
    check_target(workspace, target)?; // @lfy def/mcp/main.lfy:output#output:output:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
    // The name is the path of an output file, so its regions are the markers it holds now,
    // read as acceptance reads them: a compiler that has just edited the file sees its
    // regions where they are rather than where the last acceptance recorded them.
    // @lfy def/mcp/main.lfy:output#output:output:5f91a9d6a55f8212f9288be65dd547c69391009665dc988e76b8a5150ac20406
    if is_output_file(workspace, name, target) {
        let held = markers_now(workspace, name);
        if held.is_empty() {
            // @lfy def/mcp/main.lfy:output#output:output:7e2589822812f683a65808151f3ae1177182b076f571c65ab59fa2d7e78162a8
            return Ok(format!("nothing is generated for {name}"));
        }
        // @lfy def/mcp/main.lfy:output#output:output:22ddcfeba7456aa91005e6aeab2fcd871e9c512ede2849961438a5b7a3cc5d7c
        let blocks: Vec<String> = held
            .iter()
            .map(|marker| region_block(workspace, name, marker, text))
            .collect();
        return Ok(blocks.join("\n\n"));
    }
    let maps = source_maps_of(session, None); // @lfy def/mcp/main.lfy:output#output:output:18bbd05acd9d7ef32f170d4713899aaf2e4c19f93da25c7feae2938e50414fc1
    let every = every_region(&maps, target);
    // The name is one an entity owns, so the regions are the ones `regions_of` gives for it;
    // a marker is matched by the same target and the same marker it was found by, because
    // `regions_of` keeps no path.
    // @lfy def/mcp/main.lfy:output#output:output:d930026963bc74a22a71de25fff3d4d8ac8a7b00ff6209a9e7d4afbbff550eb1
    let owned = generation::regions_of(workspace, &maps, name, target);
    let mut regions: Vec<(&str, &Marker)> =
        every.iter().copied().filter(|(_, marker)| owned.contains(marker)).collect();
    // The name is the id of a criterion or test of the lowered program, local or global, so
    // its text comes first and the regions are the ones answering for it.
    // @lfy def/mcp/main.lfy:output#output:output:8ac14ea384bf2a5e4cb895bab8105f34dfe6124df0ef5ae67edcf3fba77a408d
    let mut heading = None;
    if regions.is_empty() {
        let mut lowered = None;
        let program = match &session.binding {
            Some(binding) => &binding.program,
            None => lowered.insert(interpret::lower(workspace.clone())),
        };
        if let Some(spelled) = requirement_text(program, name) {
            heading = Some(spelled);
            regions = every
                .iter()
                .copied()
                .filter(|(_, marker)| marker.requirement.as_deref() == Some(name))
                .collect();
        }
    }
    let mut blocks: Vec<String> = heading.into_iter().collect();
    if regions.is_empty() {
        // @lfy def/mcp/main.lfy:output#output:output:7e2589822812f683a65808151f3ae1177182b076f571c65ab59fa2d7e78162a8
        blocks.push(format!("nothing is generated for {name}"));
    }
    // @lfy def/mcp/main.lfy:output#output:output:22ddcfeba7456aa91005e6aeab2fcd871e9c512ede2849961438a5b7a3cc5d7c
    blocks.extend(
        regions
            .into_iter()
            .map(|(output, marker)| region_block(workspace, output, marker, text)),
    );
    Ok(blocks.join("\n\n"))
}

/// One lowered criterion as one line: `When` and its situations, then its behaviors, then
/// `Side effects:` and its side effects, joined by `: `, as a request and a review spell it.
// @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
fn lowered_criterion_text(criterion: &LoweredCriterion) -> String {
    let mut parts = Vec::new();
    if let Some(situation) = &criterion.situation {
        parts.push(format!("When {}", situation.join(" ")));
    }
    if let Some(behavior) = &criterion.behavior {
        parts.push(behavior.join(" "));
    }
    if let Some(side_effects) = &criterion.side_effects {
        parts.push(format!("Side effects: {}", side_effects.join(" ")));
    }
    parts.join(": ")
}

/// Every criterion and test of a lowered node and the nodes under it, with those of the
/// criteria and tests given, as its id and the one line it is spelled as.
// @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
fn collect_requirements(
    criteria: &[LoweredCriterion],
    tests: &[LoweredTest],
    out: &mut Vec<(String, String)>,
) {
    out.extend(criteria.iter().map(|criterion| (criterion.id.clone(), lowered_criterion_text(criterion))));
    out.extend(tests.iter().map(|test| (test.id.clone(), lowered_test_text(test))));
}

/// Every criterion and test of the lowered program, each as its id and the one line it is
/// spelled as: the local ones of every file, then the global ones.
// @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
pub(crate) fn requirements_of(program: &Program) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for lowered in &program.files {
        let mut nodes: Vec<&LoweredNode> = vec![&lowered.root];
        while let Some(node) = nodes.pop() {
            collect_requirements(&node.criteria, &node.tests, &mut found);
            nodes.extend(node.nodes());
        }
        collect_requirements(&lowered.criteria, &lowered.tests, &mut found);
    }
    // A global criterion or test is in the program once, under no file.
    // @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
    collect_requirements(&program.criteria, &program.tests, &mut found);
    found
}

/// The text of the criterion or test one requirement id names, local or global, from the
/// lowered program; `None` when the program holds none with that id.
// @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
fn requirement_text(program: &Program, id: &str) -> Option<String> {
    requirements_of(program)
        .into_iter()
        .find(|(spelled, _)| spelled == id)
        .map(|(_, text)| text)
}

/// Where one line of generated code came from: the definition file, the entity, and its
/// line.
// @lfy def/mcp/main.lfy:source#source:source:7095bd8049d91a01f9b2a650bf5e71d17bb706c9f4e6a470e004c2dd9881494e
pub fn source(session: &Session, file: &str, line: usize) -> Result<String, String> {
    let workspace = &session.workspace;
    for map in source_maps_of(session, None) {
        if map.output != file {
            continue;
        }
        // The marker read is the one whose region begins at or before the line and ends at
        // or after it.
        // @lfy def/mcp/main.lfy:source#source:source:52459b54250cb2edeb5d2b3079e663f17b6814b8d50602682f49ab2f55bf2285
        let Some(marker) = map.markers.iter().find(|marker| marker.covers(line)) else {
            continue;
        };
        // @lfy def/mcp/main.lfy:source#source:source:7095bd8049d91a01f9b2a650bf5e71d17bb706c9f4e6a470e004c2dd9881494e
        let mut text = format!("{}:{}", marker.file, marker.line);
        // @lfy def/mcp/main.lfy:source#source:source:b4c0ce9250c99f427b67858ad3e1b065b76f259ad3bf94c84085ee1551d51ab1
        if let Some(entity) = &marker.entity {
            text.push_str(&format!(" {entity}"));
        }
        // The requirement follows on the line after, and the text of that criterion or test
        // on the line after it; a criterion reads as a sentence, so it stands on its own
        // line rather than beside an id that is a name and a hash.
        // @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
        if let Some(requirement) = &marker.requirement {
            text.push('\n');
            text.push_str(requirement);
            let program = interpret::lower(workspace.clone());
            // A stale map may name an id the program no longer holds; the id still says
            // which requirement the region answered for.
            if let Some(spelled) = requirement_text(&program, requirement) {
                text.push('\n');
                text.push_str(&spelled);
            }
        }
        return Ok(text);
    }
    // @lfy def/mcp/main.lfy:source#source:source:3bd573d9905fc8bbb2ff9b436ef7c2affcef119a2a9cf64efa242292f4bf0299
    Ok(format!("no source map covers {file}:{line}"))
}

// ---- searching the outputs ----------------------------------------------------------

// Decision: the regular expressions are those of the Rust regex crate, the library the
// compiler itself is written with, so every search ends in time linear in the text.

/// How many matching lines a search gives before the rest are only counted.
// @lfy def/mcp/main.lfy:search#search:search:a1cf2a0988555448b5857efc090387808f6292fa53e678aeff920abc5253a1b5
pub(crate) const SEARCHED_LINES: usize = 200;

/// The files a search reads: every output file of every target it searches, each once, in
/// the order of their paths.
// @lfy def/mcp/main.lfy:search#search:search:ad139fbdabb5d3b024a7e78d751dcf2f3b3868f73d92f6746f13ae3b0356182e
fn searched_files(session: &Session) -> Vec<String> {
    let workspace = &session.workspace;
    // @lfy def/mcp/main.lfy:search#search:search:ad139fbdabb5d3b024a7e78d751dcf2f3b3868f73d92f6746f13ae3b0356182e
    let targets = session_targets(workspace, session.binding.as_ref());
    let mut found: Vec<String> = targets
        .iter()
        .flat_map(|target| output_files(workspace, target))
        .collect();
    found.sort();
    found.dedup();
    found
}

/// The lines of generated code that match a regular expression.
// @lfy def/mcp/main.lfy:search#search:search:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
pub fn search(session: &Session, pattern: &str, file: Option<&str>) -> Result<String, String> {
    let workspace = &session.workspace;
    let mut files = searched_files(session);
    // @lfy def/mcp/main.lfy:search#search:search:6fee06b6e4e8b823d41f3b760f75a448a7c9357ea1409123aaf2b2b62685f6ab
    if let Some(under) = file {
        let kept: Vec<String> = files
            .iter()
            .filter(|path| path.as_str() == under || path.starts_with(&format!("{under}/")))
            .cloned()
            .collect();
        if kept.is_empty() {
            // @lfy def/mcp/main.lfy:search#search:search:53b64d09870e3da2a2a74debb3f5058f60fda381188b78182eb74ba5aa2269f1
            return Err(format!("{under} is not under an output directory of this session's targets"));
        }
        files = kept;
    }
    // @lfy def/mcp/main.lfy:search#search:search:b52b7d681a1ab14d16e4367a42f8fe72e5a2c4577ef0e113b5f4e7a1e458ab91
    let expression = regex::Regex::new(pattern)
        // @lfy def/mcp/main.lfy:search#search:search:3eb91d30c58a23ec68143fc70f48b65844f317e8c7607db0b879df80b296c435
        .map_err(|error| format!("{pattern} is not a regular expression: {error}"))?;
    let searched = files.len();
    let mut found: Vec<String> = Vec::new();
    // The files come in the order of their paths, compared by Unicode code point, and each
    // file's lines in line order.
    // @lfy def/mcp/main.lfy:search#search:search:7841eec414d572a807f6ede13000e8a87220e884b36378adb2feef0da752b8ce
    for path in files {
        // @lfy def/mcp/main.lfy:search#search:search:54828c880e975ee7dc7220ac4d01db7f73b0a478d11a232ff914941398b8ac18
        let Ok(text) = fs::read_to_string(workspace.root.join(&path)) else { continue };
        let markers = markers_now(workspace, &path);
        for (index, line) in text.lines().enumerate() {
            // @lfy def/mcp/main.lfy:search#search:search:713960fcdedc766c95bed050ea8ded3ab14967c01ec4d36ecb5698a162d92fdd
            if !expression.is_match(line) {
                continue;
            }
            let number = index + 1;
            // @lfy def/mcp/main.lfy:search#search:search:fa10d4c06de0e21bae6488a493c889c66e5ea9d22f426fbcf0488df26d15e041
            let region = markers
                .iter()
                .rev()
                .find(|marker| marker.output_line <= number)
                .map(marker_text)
                // @lfy def/mcp/main.lfy:search#search:search:b08361d90ea78257dde207e7d32417d0b3458ac1edf1f2c1061ddfb9fcfaa2cd
                .unwrap_or_else(|| "-".to_string());
            // @lfy def/mcp/main.lfy:search#search:search:8112c25cc568699c5b153fd7ada26062fb8732b7fcaefef35a49e6389cc0a8ff
            found.push(format!("{path}:{number} [{region}]: {}", shortened(line, KEPT_WIDTH)));
        }
    }
    if found.is_empty() {
        // @lfy def/mcp/main.lfy:search#search:search:5fcbe5fc50d6c7ffeea79876a5cf547599a86c0eb99cb0bac1ed79367eee2402
        return Ok(format!("no line of the {searched} files searched matches {pattern}"));
    }
    // @lfy def/mcp/main.lfy:search#search:search:a1cf2a0988555448b5857efc090387808f6292fa53e678aeff920abc5253a1b5
    if found.len() > SEARCHED_LINES {
        let left = found.len() - SEARCHED_LINES;
        found.truncate(SEARCHED_LINES);
        found.push(format!(
            "{left} more matching lines are left out; name a file or narrow the pattern"
        ));
    }
    Ok(found.join("\n"))
}

