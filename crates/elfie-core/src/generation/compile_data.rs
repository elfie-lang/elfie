// @lfy def/generation/main.lfy:sourceMapsOf
//! The compile-time data of a project: source maps and review records.

use super::*;

/// Where the compile-time data of a project lives, under the workspace root.
// Decision: compile-time data lives in `elfie-compile` under the workspace root, never
// under an output directory, so outputs hold only generated code. `elfie-compile/maps`
// holds the source maps, one file per unit, so accepting a unit writes one small file
// instead of rewriting every map of the target, and a commit shows exactly which units a
// compile touched.
// @lfy def/generation/main.lfy:sourceMapsOf
const MAPS: &str = "elfie-compile/maps";
/// What earlier versions recorded every map of a target in, under its output directory.
// @lfy def/generation/main.lfy:sourceMapsOf
const LEGACY_MAPS: &str = "source-map.json";
/// Where the review records of a project live, under the workspace root, one file per unit
/// beside its map file.
// Decision: review records live beside the maps in `elfie-compile` and are checked in,
// because a verdict cannot be rebuilt from the definitions, the maps, and the outputs
// without a verifier.
// @lfy def/generation/main.lfy:reviewsOf
const REVIEWS: &str = "elfie-compile/reviews";

/// Every source map recorded for a workspace whose output still exists.
///
/// A unit's map file is `elfie-compile/maps` under the workspace root, then the target's
/// identifier, then the unit's stem with the extension `.json`, such as
/// `elfie-compile/maps/rust/cli/main.json` for `def/cli/main.lfy`; it holds the unit's
/// source maps as one JSON list. The maps come in the order of the workspace's targets,
/// then in the sorted order of the map files under each target's folder, then in the order
/// each file lists them. `target` is the identifier of one target, or `None` for every
/// target; when it is set, the map files of other targets are never opened. A target with
/// no folder under `elfie-compile/maps` is read from `source-map.json` under its output
/// directory instead, as earlier versions recorded every map of a target, so a project
/// keeps its maps until the next compile moves them. A map file that cannot be read, or is
/// not a JSON list of source maps, contributes nothing, so its unit plans as fresh.
/// Nothing is written, and nothing under `elfie-compile/cache` is read.
// @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
// @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:1dc22603643b17fad4874110ef09a3391b67d71c28a9f5b35a7dfa18438f3409
pub fn source_maps_of(workspace: &Workspace, target: Option<&str>) -> Vec<SourceMap> {
    let mut out: Vec<SourceMap> = Vec::new();
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:e3ab53d40653267f2e160d4a7f3251350dc8120cab5217a982112c74085d6a81
    for named in &workspace.targets {
        // A target other than the one asked for is skipped before its folder is opened, so
        // neither its order nor a map file of its own that cannot be read changes the result.
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:3fa1873c256699bcc68b376b528e4f35f840fe93aad209cc0f992a5dee017fce
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:4790ed9f6184ca4bd2972e5ec7a734acb1266c6d095e7edebd27bf8dd48cf71b
        if target.is_some_and(|target| named.identifier != target) {
            continue;
        }
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:48e0f71f7db0725ebe634eeac3dac1d913b250a6626d34b72282e59198837589
        let folder = workspace.root.join(MAPS).join(&named.identifier);
        let files = if folder.is_dir() {
            map_files(&folder) // @lfy def/generation/main.lfy:sourceMapsOf
        } else {
            // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:c968c8acaf4d38cdd8430f58ae5b09a27d1e2e6bd4ee8861bd63a620ddfd7a9c
            let legacy = workspace
                .root
                .join(&named.output_directory)
                .join(LEGACY_MAPS);
            if legacy.is_file() {
                vec![legacy]
            } else {
                // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:55c87b019cd4d2d7c4a69761ea81c9bd9980381c22407c0e202211f03cfe8dc8
                Vec::new()
            }
        };
        for file in files {
            // A file that cannot be read, or that is no JSON list of source maps,
            // contributes nothing; one that is read gives its maps in its own order, and a
            // map that records no answered ids, or no hash of its output's text, is read
            // with none.
            // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:ceffdad2c9ea38bce966887b7756582eba5aca5852cb5980c55faa063210bd93
            // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:00a062bda31894d720907cb0ec23e4bc5dd3e4d0db76200eab3b747b849ca38d
            // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:9a20ac7072bf79bbb2d38d819ba4fb8127d258b6f0baf9134dafc7c98398e94a
            // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:3b24f86bf0b585aead2fe1155f89fe18db2e2dc7209da1f6b2d48b9fb06b5619
            for map in read_source_maps(&file) {
                // A map whose output is gone says nothing about a unit that is up to date.
                // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:777506c6ffecf859d5436778cd8e1997eef47cd57e5f8f41754058bbe3117d23
                if workspace.root.join(&map.output).exists() {
                    out.push(map);
                }
            }
        }
    }
    out
}

/// Every `.json` file under a target's folder of map files, in the ascending text order of
/// their paths relative to that folder, the segments separated by `/`.
// Decision: a unit's stem holds the directories it sits in, so its map file does too and
// the folder is walked whole; the order is that of the whole relative paths as text, the
// order the loader lists a directory of source files in, so a map file beside a directory
// of the same name sorts where its own path falls rather than after the directory's files.
// @lfy def/generation/main.lfy:sourceMapsOf
fn map_files(folder: &Path) -> Vec<PathBuf> {
    let mut relative: Vec<String> = Vec::new();
    collect_map_files(folder, "", &mut relative);
    relative.sort();
    relative.iter().map(|path| folder.join(path)).collect()
}

/// Every `.json` file at or under one directory, as a path relative to the folder the walk
/// began in with `/` between the segments, in whatever order the directories give them.
// @lfy def/generation/main.lfy:sourceMapsOf
fn collect_map_files(folder: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = format!("{prefix}{name}");
        let path = entry.path();
        if path.is_dir() {
            collect_map_files(&path, &format!("{relative}/"), out);
        } else if name.ends_with(".json") {
            out.push(relative);
        }
    }
}

/// The map file of one unit: `elfie-compile/maps`, the target's identifier, and the unit's
/// stem with the extension `.json`, under the workspace root.
// @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:48e0f71f7db0725ebe634eeac3dac1d913b250a6626d34b72282e59198837589
pub fn map_file(workspace: &Workspace, unit: &Unit) -> PathBuf {
    workspace
        .root
        .join(MAPS)
        .join(&workspace.targets[unit.target].identifier)
        .join(format!("{}.json", unit.stem))
}

/// Whether the source maps of one accepted unit are recorded in its map file.
///
/// The file holds them afterwards, replacing whatever it held, and no other file is
/// written, so recording one unit leaves every other map file byte for byte as it was. It
/// is the pretty JSON of the maps in output order, each map's markers in output order and
/// the keys of every object in alphabetical order, so the same maps always give the same
/// bytes and a diff shows only what changed. Maps that differ from what the file holds only
/// in when they were generated leave the file as it is, so accepting outputs that did not
/// change changes nothing on disk. Every missing folder above the file is created, so a
/// project cloned without `elfie-compile` records its maps; no source map at all removes
/// the file, so the unit plans as fresh. `false` when the file cannot be written.
// Only the unit's own map file is written, so recording one unit leaves every other byte
// for byte as it was.
// @lfy def/generation/main.lfy:record#record:record:d7ed944c0449f01835fd94e7de01e58066347177591b374f6a7beee48113f403
// @lfy def/generation/main.lfy:record#record:record:8d039cf20453fa3420e6acdb3ce42b2031909a9bc211b646a683be5017efa7fd
pub fn record(workspace: &Workspace, unit: &Unit, source_maps: &[SourceMap]) -> bool {
    let path = map_file(workspace, unit);
    if source_maps.is_empty() {
        // @lfy def/generation/main.lfy:record#record:record:aa6df37cdc9d7a159ba508148db794c3edbc568247899ccbd55574df85a1177d
        return match std::fs::remove_file(&path) {
            Ok(()) => true,
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        };
    }
    // @lfy def/generation/main.lfy:record#record:record:5b5e70df30f90156b3023da573235ca899c7a55801fbecd76167819cf57e21ad
    let mut ordered: Vec<SourceMap> = source_maps.to_vec();
    ordered.sort_by(|left, right| left.output.cmp(&right.output));
    // The readable layout of the maps, with the keys of every object in alphabetical order,
    // so the same maps always give the same bytes and a diff shows only what changed.
    // @lfy def/generation/main.lfy:record#record:record:1a8586a7f2168dcbbbc34c4373d1f944d6e0313f9b0840a53aa8f54e3e3208d6
    // @lfy def/generation/main.lfy:record#record:record:60d0cd914ebe3c1cbbd582cd3229d8adc3a00b879720f1caeec671fb4c707551
    // @lfy def/generation/main.lfy:record#record:record:5c0c63725b35e7fab925c81abc27914b55863257ede7a2863a3bce15e5b03ff5
    let value = serde_json::Value::Array(ordered.iter().map(SourceMap::to_json).collect());
    let Ok(mut text) = serde_json::to_string_pretty(&value) else {
        return false;
    };
    text.push('\n');
    // Maps that differ only in when they were generated are the maps the file holds.
    // @lfy def/generation/main.lfy:record#record:record:acccd867dc43a326a46b14f88ac19bb24373ec1a90795894fa96f075e1fcbcd9
    let recorded = read_source_maps(&path);
    if recorded.len() == ordered.len()
        && recorded.iter().zip(&ordered).all(|(held, map)| {
            *held
                == SourceMap {
                    generated: held.generated.clone(),
                    ..map.clone()
                }
        })
    {
        return true;
    }
    // @lfy def/generation/main.lfy:record#record:record:1ec300497e4609fbc621214de54b44c8b7f666382b4cf02496efdc55ed4cca99
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return false;
    }
    // @lfy def/generation/main.lfy:record#record:record:1f4671d2ae5027d4e5bcfaf56dc68c765e27e31a92fc783b7f64a2165f391dd4
    std::fs::write(&path, text).is_ok()
}

// ---------------------------------------------------------------------------------------
// rederive
// ---------------------------------------------------------------------------------------

/// The source maps of a unit as the files of its target's output directory hold its markers
/// now, carrying over what its last accepted generation recorded.
///
/// One map per file of the output directory, recursively, that holds a marker of this unit's
/// own, in path order; its markers are every marker of that file naming a file of the
/// program, read and derived as acceptance reads and derives one and left as written. What
/// the last accepted generation recorded is carried over output by output: a file the unit
/// already had a map for keeps that map's hash, requirements, answered ids, signature, text
/// hash, dependency signatures, and time, so what the unit was generated from, and what
/// decides whether it is planned, stays as it was. A file it had no map for — one another
/// unit's split moved its regions into — takes the six the unit's maps agree on and an empty
/// text hash, since no text of that file was ever accepted. Where those six are not the same
/// across every map, there is nothing to carry and the maps come back as they are, as they do
/// when the unit has no maps or no file of the output directory holds a marker of its own.
/// Nothing is written, and no compiler, verifier, or command of the target is run.
// Decision: the definition passes the workspace and the unit; a plan here does not own its
// workspace or the program lowered from it, so the program and the plan are extra first
// parameters and the unit is given as its index into the plan.
// Decision: `SourceMap.text` is carried over and never read again from the files: it is the
// hash of the code the unit's own acceptance and review saw, so another unit's unreviewed
// edit inside one of its regions, or lines put under one of its markers, leaves its outputs
// no longer matching it.
// @lfy def/generation/main.lfy:rederive
// @lfy def/generation/main.lfy:rederive#rederive:rederive:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
// @lfy def/generation/main.lfy:rederive#rederive:rederive:31a78a92552eb31060ad0493c622169b65add6ccf8f0055fb092520eccf37286
pub fn rederive(
    program: &Program,
    plan: &Plan,
    unit: usize,
    source_maps: &[SourceMap],
) -> Vec<SourceMap> {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let planned = &plan.units[unit];
    let target = &workspace.targets[planned.target];
    let file = &workspace.files[planned.file];
    // @lfy def/generation/main.lfy:rederive#rederive:rederive:ab3fc93517c19e9574351a5b75cd5adfdb9863d5af006fbb2bf0cac32ef94646
    if source_maps.is_empty() {
        return source_maps.to_vec();
    }
    // Every file of the output directory, recursively, in the ascending text order of its
    // path relative to the root.
    // @lfy def/generation/main.lfy:rederive#rederive:rederive:33f6e3ae8b3a61bf5f205818bb8797ab2d0475dc7323d5212b3b38734b4ae594
    let mut paths: Vec<String> = Vec::new();
    collect_output_files(
        &workspace.root.join(&target.output_directory),
        &target.output_directory,
        &mut paths,
    );
    paths.sort();

    let named = named_entities(model, file.source, &planned.entities);
    // The files whose markers this unit may name a line of, as acceptance claims them: its
    // own, and the file of each entity moved into it.
    // @lfy def/generation/main.lfy:rederive
    let mut claimed: Vec<&str> = vec![&file.path];
    for moved in plan.moves.iter().filter(|moved| moved.into == unit) {
        let path = &workspace.files[plan.units[moved.origin].file].path;
        if !claimed.contains(&path.as_str()) {
            claimed.push(path);
        }
    }
    // Each file that holds a marker of this unit's own, with every marker of it that names a
    // file of the program, read and derived as acceptance does and left as written.
    // @lfy def/generation/main.lfy:rederive#rederive:rederive:33f6e3ae8b3a61bf5f205818bb8797ab2d0475dc7323d5212b3b38734b4ae594
    // @lfy def/generation/main.lfy:rederive#rederive:rederive:de0c92c8e9054b77050411f99aab03406cef407c814ca97ee7ab8181bdebf960
    let mut found: Vec<(String, Vec<Marker>)> = Vec::new();
    for path in paths {
        let Ok(text) = std::fs::read_to_string(workspace.root.join(&path)) else {
            continue;
        };
        let output = Output {
            path: path.clone(),
            text,
        };
        // A problem acceptance would report is none of this: nothing here judges an output,
        // it only reads where the regions are now.
        let mut ignored: Vec<String> = Vec::new();
        let markers: Vec<Marker> = parse_markers(&output.text)
            .iter()
            .filter_map(|marker| {
                resolve_marker(
                    workspace,
                    file,
                    &named,
                    marker,
                    &claimed,
                    &output,
                    &mut ignored,
                )
            })
            .collect();
        if markers
            .iter()
            .any(|marker| is_own_marker(workspace, plan, unit, marker))
        {
            found.push((output.path, markers));
        }
    }
    // @lfy def/generation/main.lfy:rederive#rederive:rederive:ab3fc93517c19e9574351a5b75cd5adfdb9863d5af006fbb2bf0cac32ef94646
    if found.is_empty() {
        return source_maps.to_vec();
    }
    // What the last accepted generation recorded, for a file the unit had no map for: the six
    // every map of the unit agrees on, since each of them is of the unit rather than of one
    // output. Where two maps differ in any of them, there is nothing to carry.
    // @lfy def/generation/main.lfy:rederive#rederive:rederive:838d2f8ee921d0ec16aab7f04aebafa5071a0f529f8cf1a8a06ed85f0a4e5149
    // @lfy def/generation/main.lfy:rederive#rederive:rederive:0ed7b6723ce64cd6cf1e09f24d4516bf399f45d394b5cb0e44753e1df9b7fb01
    let first = &source_maps[0];
    let agreed = source_maps.iter().all(|map| {
        map.hash == first.hash
            && map.requirements == first.requirements
            && map.answered == first.answered
            && map.signature == first.signature
            && map.dependencies == first.dependencies
            && map.generated == first.generated
    });
    if !agreed
        && found
            .iter()
            .any(|(path, _)| !source_maps.iter().any(|map| &map.output == path))
    {
        return source_maps.to_vec();
    }
    found
        .into_iter()
        .map(|(path, markers)| {
            // @lfy def/generation/main.lfy:rederive#rederive:rederive:7656b04818cc881ae3204287ce2ad06c3b43ae1293a39033fad0f9eadb054f2b
            let carried = source_maps
                .iter()
                .find(|map| map.output == path)
                .unwrap_or(first);
            // A file the unit had no map for has no text that was ever accepted, so its text
            // hash is empty, which no hash of a region text equals.
            let text = if carried.output == path {
                carried.text.clone()
            } else {
                String::new()
            };
            SourceMap {
                // @lfy def/generation/main.lfy:rederive#rederive:rederive:17ed6196ab28af701579b3e34153efce5461c44c096f7133d7967b685c4b53c4
                target: target.identifier.clone(),
                output: path,
                source: file.path.clone(),
                hash: carried.hash.clone(),
                requirements: carried.requirements.clone(),
                answered: carried.answered.clone(),
                signature: carried.signature.clone(),
                text,
                dependencies: carried.dependencies.clone(),
                generated: carried.generated.clone(),
                markers,
            }
        })
        .collect()
}

/// Every file at or under one directory, as a path relative to the workspace root with `/`
/// between the segments, in whatever order the directories give them.
// @lfy def/generation/main.lfy:rederive
fn collect_output_files(directory: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = under_root(prefix, &name);
        let path = entry.path();
        if path.is_dir() {
            collect_output_files(&path, &relative, out);
        } else {
            out.push(relative);
        }
    }
}

// ---------------------------------------------------------------------------------------
// reviewsOf, recordReviews
// ---------------------------------------------------------------------------------------

/// The review record file of one unit: `elfie-compile/reviews`, the target's identifier, and
/// the unit's stem with the extension `.json`, under the workspace root, such as
/// `elfie-compile/reviews/rust/cli/main.json` for `def/cli/main.lfy`.
// @lfy def/generation/data.lfy:ReviewRecord#ReviewRecord:ReviewRecord:d1283130451aac07da56acfa5179b7af277f4cbbb6d93ed00c6b0aad6499db7a
pub fn review_file(workspace: &Workspace, unit: &Unit) -> PathBuf {
    workspace
        .root
        .join(REVIEWS)
        .join(&workspace.targets[unit.target].identifier)
        .join(format!("{}.json", unit.stem))
}

/// The review records of one unit, as its review record file holds them.
///
/// The file holds them as one JSON list, and they come back in the order it lists them. A
/// file that is missing, that cannot be read, or that is no JSON list of review records
/// gives none, so every criterion and test of the unit is reviewed again. A record whose id
/// is the id of no local criterion or test of the unit is left out, so a criterion that was
/// reworded is never carried on the verdict its old id earned. Nothing is written, and
/// nothing under `elfie-compile/cache` is read.
// Decision: the definition passes the workspace; the ids of a unit's local criteria and
// tests are the lowered program's, which a plan here does not own, so the program stands in
// for the workspace as it does for [`changes`].
// @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
// @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:1dc22603643b17fad4874110ef09a3391b67d71c28a9f5b35a7dfa18438f3409
pub fn reviews_of(program: &Program, unit: &Unit) -> Vec<ReviewRecord> {
    let path = review_file(&program.workspace, unit);
    // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:0aa4f9eaff20fee7bf263db985597b4d886a2ac762b47b1af1734107a9ac1503
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    let Some(records) = items
        .iter()
        .map(ReviewRecord::from_json)
        .collect::<Option<Vec<ReviewRecord>>>()
    else {
        return Vec::new();
    };
    let local: HashSet<String> = unit
        .requirements(program)
        .iter()
        .map(|requirement| requirement.id().to_string())
        .collect();
    // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:d95399b23b29d50a8fba5af56c20731f8f7b0b2bafe16e9914d5090e374ce251
    records
        .into_iter()
        // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:0b41caebdc61220c67f088bc69f465267b2c9e2b36d07c52b02680bf6cb4724c
        .filter(|record| local.contains(&record.id))
        .collect()
}

/// Whether the review records of one unit are recorded in its review record file.
///
/// The file holds them afterwards, replacing whatever it held, and no other file is written.
/// It is the pretty JSON of the records in the order they were given, with the keys of every
/// object in alphabetical order, so the same records always give the same bytes and a diff
/// shows only what changed. Every missing folder above the file is created, so a project
/// cloned without `elfie-compile` records its verdicts; no record at all removes the file,
/// so every criterion of the unit is reviewed again. `false` when the file cannot be
/// written.
// @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:5708abb1bf089eb06657f5688610a6c234b81463164fd199b77b315608316837
// @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:8d039cf20453fa3420e6acdb3ce42b2031909a9bc211b646a683be5017efa7fd
pub fn record_reviews(workspace: &Workspace, unit: &Unit, records: &[ReviewRecord]) -> bool {
    let path = review_file(workspace, unit);
    if records.is_empty() {
        // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:0149e6a1c5272af0b7ca76e627e0fb18e511db4e2076f128db7607e14f49ae85
        return match std::fs::remove_file(&path) {
            Ok(()) => true,
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        };
    }
    // The readable layout of the records, in the order they were given, with the keys of
    // every object in alphabetical order, so the same records always give the same bytes.
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:52d12769dcd44be836a5afeb17b9c7341a2a82265bafc029ee658a417f8e0569
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:c0e143aef64c3c7bed3e5892dd6d03a0c46df17166ce5ffa598b2db167027369
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:60d0cd914ebe3c1cbbd582cd3229d8adc3a00b879720f1caeec671fb4c707551
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:db1f607aac959c64937cb811664557dedde402eb3fdcaaaf6a2db6b1284ecfd1
    let value = serde_json::Value::Array(records.iter().map(ReviewRecord::to_json).collect());
    let Ok(mut text) = serde_json::to_string_pretty(&value) else {
        return false;
    };
    text.push('\n');
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:72846edcc746900edf9c781c66023ad863b10442a8c62d330bee15766a2090c3
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return false;
    }
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:1f4671d2ae5027d4e5bcfaf56dc68c765e27e31a92fc783b7f64a2165f391dd4
    std::fs::write(&path, text).is_ok()
}
