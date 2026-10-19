// @lfy def/generation/previous.lfy:previousOf
//! Compiled from `def/generation/previous.lfy`: the source of a unit at its last accepted
//! generation, recovered once from the working tree or from git, and lowered once.
//!
//! [`previous_of`] is reached as `generation::previous::previous_of`, apart from the
//! `generation::previous_of` that only builds a [`Previous`] from a text it is handed.

use std::path::Path;
use std::process::{Command, Stdio};

use super::{
    Previous, SourceMap, Unit, answered_ids, built_entities, lowered_code, lowered_declaration,
    lowered_node_of, named_entities, qualified_name, declaring_path, source_hash,
};
use crate::interpret::{Program, lower};
use crate::model::EntityId;
use crate::workspace::change;

/// The source of a unit at its last accepted generation, or else its hash-matched source,
/// with the program lowered from it; `None` when neither can be recovered.
///
/// `program` is the program the unit was planned from, and `source_maps` the source maps
/// recorded for the unit, such as its [`Unit::outputs`]. The unit's file as `program` holds
/// it is tried first, and only when it is not that source are the revisions git holds of
/// it read, newest first, each lowered at most once. Nothing is written.
// @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
// @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:31a78a92552eb31060ad0493c622169b65add6ccf8f0055fb092520eccf37286
pub fn previous_of(program: &Program, unit: &Unit, source_maps: &[SourceMap]) -> Option<Previous> {
    // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:3c07e6c0052baae979054c9a0caa0b4293a8750c5e525360546300a75f588bd9
    // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:a572cd8671d623e73cf927abcaa85b67de94095ac5c9348450a02277bcbf737e
    let map = source_maps.first()?;
    let workspace = &program.workspace;
    let path = workspace.files[unit.file].path.clone();
    // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:0e70a89a209446af9c519b3ccbe86315969889346aad01e3716c2825862101e4
    // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:0fd69784cc9066c30ec9db217b85ccfda97feeb482053e9af8668f5bb0270b2d
    if is_accepted_source(map, &unit.text, &answered_ids(program, unit)) {
        let text = std::fs::read_to_string(workspace.root.join(Path::new(&path))).ok()?;
        return Some(Previous {
            text,
            program: program.clone(),
            hash_matched: false,
        });
    }
    // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:c879ef7aff354c258ba879c3bedc8bb550fb2cc4cde07af2658db79c26265e85
    let revisions = revisions(&workspace.root, &path)?;
    // The newest revision whose lowered text hashes to the map's, kept in case no revision
    // is the source at the last accepted generation.
    // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:64a49b01c60baf19116b3d2278ef7e70af6083fe469e56f5753e9add84d31c62
    let mut matched: Option<Previous> = None;
    // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:02cf361d605c1d4d09a099387a26aac60cde3bec2d428719c8bb279e36b6c925
    for revision in revisions {
        // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:ffdc918b7c5b54450a5d8740507876a1db872636b7b424d57872162e2a624880
        let Some(text) = shown(&workspace.root, &revision, &path) else {
            continue;
        };
        // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:fed61b026aa9ba10c0bb77a48f0f8a5a52ef3e098da5707ccef5261284f12c7e
        let lowered = lower(change(workspace, &path, Some(&text)));
        let Some((lowered_text, ids)) = lowered_of(program, unit, &lowered, &path) else {
            continue;
        };
        if source_hash(&lowered_text) != map.hash {
            continue;
        }
        // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:c02714f1d22eb8c5b6179cbac95cf2c59da1c2e5e0a14b45fbd32eb12182e4eb
        if ids_match(map, &ids) {
            return Some(Previous {
                text,
                program: lowered,
                hash_matched: false,
            });
        }
        // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:12d39246795349a3c9f83f034c4fc79746a255adb1fb2ec60b04e1c3e9b25ba8
        if matched.is_none() {
            matched = Some(Previous {
                text,
                program: lowered,
                hash_matched: true,
            });
        }
    }
    // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:a06403aea8057b333ac4ccc3929c1b5604d4ca8e44ef9177656ed11e0924fbde
    matched
}

/// Whether a text is the source at the last accepted generation: its lowered text hashes to
/// the map's hash and its ids, sorted, to the map's requirements.
// @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:5e5ea8e03652a58d1ece1a0fbf931a3a69d52176155edcf1e68c68a027cd3b61
fn is_accepted_source(map: &SourceMap, lowered_text: &str, ids: &[String]) -> bool {
    source_hash(lowered_text) == map.hash && ids_match(map, ids)
}

/// Whether a text's ids, sorted, are the ones the map was recorded for: compared with the
/// ids the map answered when it records them, and by their hash otherwise.
// @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:cd8b17f1666e508e31a72b32115befaa858deb434bf75e476db720f403bf7b17
fn ids_match(map: &SourceMap, ids: &[String]) -> bool {
    if !map.answered.is_empty() {
        return map.answered == ids;
    }
    source_hash(&ids.join("\n")) == map.requirements
}

/// The lowered text of the unit's file in a program a revision was lowered in, made as
/// [`Unit::text`] is made, and the ids of its local criteria and tests, sorted, taken as
/// [`SourceMap::requirements`] takes them. The entities the unit moved out and in are found
/// in that program by the file that declares them and the name they go by.
// @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:9d7f4cdb0003b58ea19a1684c0862b7848e23bf07f113f64272062121d93b184
// @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:96f543d99792e3e7366c95140ea4e2ed60ac51e3679efaff597b47870d86ac77
fn lowered_of(
    program: &Program,
    unit: &Unit,
    revised: &Program,
    path: &str,
) -> Option<(String, Vec<String>)> {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let target = &workspace.targets[unit.target];
    let own_file = &program.files[unit.lowered];
    // What the unit's file builds for the target as `program` holds it, and of that what
    // moved out of the unit and what moved into it.
    let own: Vec<EntityId> = built_entities(model, workspace.files[unit.file].source, target.declaration)
        .into_iter()
        .filter(|&entity| lowered_node_of(own_file, entity).is_some())
        .collect();
    let moved_out: Vec<String> = own
        .iter()
        .filter(|entity| !unit.entities.contains(entity))
        .map(|&entity| qualified_name(model, entity))
        .collect();
    let moved_in: Vec<(String, String)> = unit
        .entities
        .iter()
        .filter(|entity| !own.contains(entity))
        .filter_map(|&entity| {
            Some((declaring_path(model, entity)?.to_string(), qualified_name(model, entity)))
        })
        .collect();

    let revised_workspace = &revised.workspace;
    let revised_model = &revised_workspace.model;
    let index = revised_workspace.files.iter().position(|file| file.path == path)?;
    let revised_target = revised_workspace
        .targets
        .iter()
        .position(|candidate| candidate.identifier == target.identifier)?;
    let revised_file = &revised.files[index];
    let (out, kept): (Vec<EntityId>, Vec<EntityId>) = built_entities(
        revised_model,
        revised_workspace.files[index].source,
        revised_workspace.targets[revised_target].declaration,
    )
    .into_iter()
    .filter(|&entity| lowered_node_of(revised_file, entity).is_some())
    .partition(|&entity| moved_out.contains(&qualified_name(revised_model, entity)));
    let into: Vec<EntityId> = moved_in
        .iter()
        .filter_map(|(declaring, name)| {
            let source = revised_workspace.file(declaring)?.source;
            named_entities(revised_model, source, &[])
                .into_iter()
                .find(|(candidate, _)| candidate == name)
                .map(|(_, entity)| entity)
        })
        .collect();
    let text = unit_text(revised, index, &out, &into);
    let revised_unit = Unit {
        target: revised_target,
        file: index,
        entities: kept.into_iter().chain(into.iter().copied()).collect(),
        lowered: index,
        ..Default::default()
    };
    Some((text, answered_ids(revised, &revised_unit)))
}

/// [`Unit::text`] of a file of a program: its lowered text without the code of the entities
/// moved out of it, then the lowered code of each entity moved into it; exactly its lowered
/// text when nothing moved in or out.
// @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:9d7f4cdb0003b58ea19a1684c0862b7848e23bf07f113f64272062121d93b184
fn unit_text(program: &Program, lowered: usize, moved_out: &[EntityId], moved_in: &[EntityId]) -> String {
    let model = &program.workspace.model;
    let file = &program.files[lowered];
    if moved_out.is_empty() && moved_in.is_empty() {
        return file.text.clone();
    }
    let mut text = file.text.clone();
    for &entity in moved_out {
        let Some(node) = lowered_node_of(file, entity) else {
            continue;
        };
        let code = lowered_code(model, node);
        let code = code.trim();
        if !code.is_empty()
            && let Some(at) = text.find(code)
        {
            text.replace_range(at..at + code.len(), "");
        }
    }
    while let Some(at) = text.find("\n\n\n") {
        text.replace_range(at..at + 1, "");
    }
    for &entity in moved_in {
        let Some(node) = lowered_declaration(program, entity) else {
            continue;
        };
        let code = lowered_code(model, node);
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        if !text.is_empty() && !text.ends_with("\n\n") {
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.push('\n');
        }
        text.push_str(code);
        text.push('\n');
    }
    text
}

/// The revisions of a file, newest first, as `git log --format=%H --` and the path lists
/// them, run in the root; `None` when the root is not a git repository or git log fails.
// @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:9510126792e8ee07262196c3b77a78b73dbab9b04e3ab2f86631d827d9172df1
// @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:c879ef7aff354c258ba879c3bedc8bb550fb2cc4cde07af2658db79c26265e85
fn revisions(root: &Path, path: &str) -> Option<Vec<String>> {
    let log = run(&format!("git log --format=%H -- {}", quoted(path)), root)?;
    Some(
        log.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// What `git show` of a revision, a colon, and a path prints, run in the root; `None` when
/// it fails.
// @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:9510126792e8ee07262196c3b77a78b73dbab9b04e3ab2f86631d827d9172df1
// @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:ffdc918b7c5b54450a5d8740507876a1db872636b7b424d57872162e2a624880
fn shown(root: &Path, revision: &str, path: &str) -> Option<String> {
    run(&format!("git show {}", quoted(&format!("{revision}:{path}"))), root)
}

/// `Process.run` of a command line in a directory, through the platform shell: what it
/// printed, decoded as UTF-8, when it exits 0; `None` when it cannot be started or fails.
// @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:9510126792e8ee07262196c3b77a78b73dbab9b04e3ab2f86631d827d9172df1
fn run(command: &str, directory: &Path) -> Option<String> {
    let output = shell(command)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The platform shell given a command line: `sh -c` on Unix.
// @lfy def/generation/previous.lfy:previousOf
#[cfg(not(windows))]
fn shell(command: &str) -> Command {
    let mut shell = Command::new("sh");
    shell.arg("-c").arg(command);
    shell
}

/// The platform shell given a command line: `cmd /C` on Windows, the line passed as written.
// @lfy def/generation/previous.lfy:previousOf
#[cfg(windows)]
fn shell(command: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut shell = Command::new("cmd");
    shell.arg("/C").raw_arg(command);
    shell
}

/// One argument quoted for the platform shell.
// @lfy def/generation/previous.lfy:previousOf
#[cfg(not(windows))]
fn quoted(argument: &str) -> String {
    format!("'{}'", argument.replace('\'', "'\\''"))
}

/// One argument quoted for the platform shell.
// @lfy def/generation/previous.lfy:previousOf
#[cfg(windows)]
fn quoted(argument: &str) -> String {
    format!("\"{}\"", argument.replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use super::super::{Fixture, current_maps, plan_of, unit_of};
    use super::*;

    /// `def/a.lfy` holding a fn with one criterion and one test, the criterion said as given.
    // @lfy def/generation/previous.lfy:previousOf
    fn a_source(behavior: &str) -> String {
        format!(
            "fn f(x: string): `Makes it` => string {{\n  @acceptanceCriteria.add({{ behavior = `{behavior}` }});\n  @test({{ input = [\"y\"], expect = string@like(`y`) }});\n}}\n"
        )
    }

    /// The same file with its code edited: a second fn declared.
    // @lfy def/generation/previous.lfy:previousOf
    fn an_edited_source() -> String {
        format!("{}\nfn g(): `Makes another` => string;\n", a_source("it makes it"))
    }

    /// Git run in a fixture's root with an identity of its own; `false` when it cannot be run
    /// or fails.
    // @lfy def/generation/previous.lfy:previousOf
    fn git(root: &Path, arguments: &[&str]) -> bool {
        Command::new("git")
            .args(["-c", "user.name=elfie", "-c", "user.email=elfie@example.com"])
            .args(["-c", "commit.gpgsign=false"])
            .args(arguments)
            .current_dir(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    /// The fixture's root made a git repository holding every file as it is now; `false`
    /// when git cannot be run here.
    // @lfy def/generation/previous.lfy:previousOf
    fn committed(fixture: &Fixture) -> bool {
        git(&fixture.root, &["init", "-q"])
            && git(&fixture.root, &["add", "-A"])
            && git(&fixture.root, &["commit", "-q", "-m", "a"])
    }

    /// The program of a fixture, the unit of `def/a.lfy`, and the maps recorded for it.
    // @lfy def/generation/previous.lfy:previousOf
    fn unit_and_maps(fixture: &Fixture) -> (Program, Unit, Vec<SourceMap>) {
        let program = fixture.program();
        let plan = plan_of(&program, &[], &[], &[]);
        let (index, unit) = unit_of(&program.workspace, &plan, "def/a.lfy");
        let maps = vec![current_maps(&program, &plan).swap_remove(index)];
        (program, unit.clone(), maps)
    }

    // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:af91be2dbe05c76be239d464b6d82e4440d787929cb23ddad100bd4ab8f155f9
    #[test]
    fn previous_of_gives_the_file_on_disk_when_it_is_the_accepted_source() {
        let fixture = Fixture::with_rust_target();
        fixture.write("def/a.lfy", &a_source("it makes it"));
        let (program, unit, maps) = unit_and_maps(&fixture);
        let previous = previous_of(&program, &unit, &maps).expect("the file on disk");
        assert_eq!(previous.text, a_source("it makes it"));
        assert_eq!(previous.program, program);
        assert!(!previous.hash_matched);
    }

    // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:3c07e6c0052baae979054c9a0caa0b4293a8750c5e525360546300a75f588bd9
    #[test]
    fn previous_of_gives_nothing_without_source_maps() {
        let fixture = Fixture::with_rust_target();
        fixture.write("def/a.lfy", &a_source("it makes it"));
        let (program, unit, _) = unit_and_maps(&fixture);
        assert_eq!(previous_of(&program, &unit, &[]), None);
    }

    // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:3b5b88293e66cca2f7b87bfff133d51446b9560280d5af0211310f4907623fbe
    #[test]
    fn previous_of_gives_the_newest_revision_that_is_the_accepted_source() {
        let fixture = Fixture::with_rust_target();
        fixture.write("def/a.lfy", &a_source("it makes it"));
        let (_, _, maps) = unit_and_maps(&fixture);
        if !committed(&fixture) {
            return; // No git here: nothing is recoverable, and nothing to check.
        }
        fixture.write("def/a.lfy", &an_edited_source());
        let (program, unit, _) = unit_and_maps(&fixture);
        let previous = previous_of(&program, &unit, &maps).expect("the committed revision");
        assert_eq!(previous.text, a_source("it makes it"));
        assert!(!previous.hash_matched);
        let index = previous
            .program
            .workspace
            .files
            .iter()
            .position(|file| file.path == "def/a.lfy")
            .unwrap();
        assert_eq!(source_hash(&previous.program.files[index].text), maps[0].hash);
        assert_ne!(previous.program, program);
    }

    // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:fa025737de883948e7befeb0b1107cabe6ceb5064f9ab924675f629af5395102
    #[test]
    fn previous_of_gives_the_hash_matched_revision_when_none_is_the_accepted_source() {
        let fixture = Fixture::with_rust_target();
        fixture.write("def/a.lfy", &a_source("it makes it"));
        if !committed(&fixture) {
            return; // No git here: nothing is recoverable, and nothing to check.
        }
        // Accepted from a rewording never committed, then reworded again on disk.
        fixture.write("def/a.lfy", &a_source("it makes it well"));
        let (_, _, maps) = unit_and_maps(&fixture);
        fixture.write("def/a.lfy", &a_source("it makes it at once"));
        let (program, unit, _) = unit_and_maps(&fixture);
        let previous = previous_of(&program, &unit, &maps).expect("the hash-matched revision");
        assert_eq!(previous.text, a_source("it makes it"));
        assert!(previous.hash_matched);
    }

    // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:461dbb4f1b2bf2e10fc49c896920863a2606b081becd66d225a315e70c16a725
    #[test]
    fn previous_of_gives_nothing_for_an_edited_file_outside_git() {
        let fixture = Fixture::with_rust_target();
        fixture.write("def/a.lfy", &a_source("it makes it"));
        let (_, _, maps) = unit_and_maps(&fixture);
        fixture.write("def/a.lfy", &an_edited_source());
        let (program, unit, _) = unit_and_maps(&fixture);
        if git(&fixture.root, &["rev-parse", "--git-dir"]) {
            return; // The temporary directory is inside a repository: not the case at hand.
        }
        assert_eq!(previous_of(&program, &unit, &maps), None);
    }

    // @lfy def/generation/previous.lfy:previousOf#previousOf:previousOf:ffdc918b7c5b54450a5d8740507876a1db872636b7b424d57872162e2a624880
    #[test]
    fn previous_of_skips_a_revision_git_cannot_show() {
        let fixture = Fixture::with_rust_target();
        fixture.write("def/a.lfy", &a_source("it makes it"));
        let (_, _, maps) = unit_and_maps(&fixture);
        if !committed(&fixture) {
            return; // No git here: nothing is recoverable, and nothing to check.
        }
        // The newest revision deletes the file, so git show fails for it.
        let _ = fs::remove_file(fixture.root.join("def/a.lfy"));
        assert!(git(&fixture.root, &["commit", "-q", "-a", "-m", "gone"]));
        fixture.write("def/a.lfy", &an_edited_source());
        let (program, unit, _) = unit_and_maps(&fixture);
        let previous = previous_of(&program, &unit, &maps).expect("the older revision");
        assert_eq!(previous.text, a_source("it makes it"));
        assert!(!previous.hash_matched);
    }
}
