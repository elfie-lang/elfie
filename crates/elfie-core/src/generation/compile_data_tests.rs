// @lfy def/generation/main.lfy:sourceMapsOf
//! Tests of the compile-time data of a project.

use super::*;

/// A unit's map file is `elfie-compile/maps`, the target's identifier, and the unit's
/// stem with the extension `.json`, under the workspace root.
// @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:48e0f71f7db0725ebe634eeac3dac1d913b250a6626d34b72282e59198837589
#[test]
fn a_units_map_file_is_under_elfie_compile_maps_by_target_and_stem() {
    let fixture = Fixture::with_rust_target();
    fixture.write("def/cli/main.lfy", "d Cli { $x = string; }\n");
    let program = fixture.program();
    assert_bound(&program.workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, unit) = unit_of(&program.workspace, &plan, "def/cli/main.lfy");
    assert_eq!(unit.stem, "cli/main");
    assert_eq!(
        map_file(&program.workspace, unit),
        fixture.root.join("elfie-compile/maps/rust/cli/main.json")
    );
}

/// Every map file of the target's folder is read, in the sorted order of its files and
/// then in the order each file lists them, and a map whose output is gone is left out.
// @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:ede8434812c8a4f5743a0062976be24f2518397279ef2820986a6880aabf5940
#[test]
fn the_maps_of_a_target_are_read_in_file_then_list_order() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("crates/a/src/a.rs", "// a\n")
        .write("crates/b/src/b.rs", "// b\n");
    write_source_maps(
        &fixture.root.join("elfie-compile/maps/rust/a.json"),
        &[map_for("crates/a/src/a.rs", "def/a.lfy")],
    )
    .unwrap();
    write_source_maps(
        &fixture.root.join("elfie-compile/maps/rust/b.json"),
        &[map_for("crates/b/src/b.rs", "def/b.lfy")],
    )
    .unwrap();
    let workspace = load(&fixture.root);
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:e3ab53d40653267f2e160d4a7f3251350dc8120cab5217a982112c74085d6a81
    let maps = source_maps_of(&workspace, None);
    assert_eq!(
        outputs_of(&maps),
        ["crates/a/src/a.rs", "crates/b/src/b.rs"]
    );
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:3fa1873c256699bcc68b376b528e4f35f840fe93aad209cc0f992a5dee017fce
    assert_eq!(outputs_of(&source_maps_of(&workspace, Some("rust"))).len(), 2);
    assert!(source_maps_of(&workspace, Some("other")).is_empty());
    // A map that records no answered ids, and no hash of its output's text, is read with
    // neither.
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:9a20ac7072bf79bbb2d38d819ba4fb8127d258b6f0baf9134dafc7c98398e94a
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:3b24f86bf0b585aead2fe1155f89fe18db2e2dc7209da1f6b2d48b9fb06b5619
    fixture.write(
        "elfie-compile/maps/rust/a.json",
        "[{\"target\":\"rust\",\"output\":\"crates/a/src/a.rs\",\"source\":\"def/a.lfy\",\
         \"hash\":\"\",\"requirements\":\"\",\"signature\":\"\",\"dependencies\":{},\
         \"generated\":\"\",\"markers\":[]}]",
    );
    let maps = source_maps_of(&workspace, Some("rust"));
    assert_eq!(maps[0].output, "crates/a/src/a.rs");
    assert!(maps[0].answered.is_empty(), "{:?}", maps[0].answered);
    assert_eq!(maps[0].text, "");
    assert_ne!(maps[0].text, source_hash("// a\n"));

    // The map of an output that is gone is left out.
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:777506c6ffecf859d5436778cd8e1997eef47cd57e5f8f41754058bbe3117d23
    fs::remove_file(fixture.root.join("crates/b/src/b.rs")).unwrap();
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:49f3bf512172a2a718b6ee96460c1a23944304c9e259e781c8928937dbb775fc
    assert_eq!(
        outputs_of(&source_maps_of(&workspace, None)),
        ["crates/a/src/a.rs"]
    );

    // A map file that cannot be read as a JSON list of source maps contributes nothing,
    // and every other map file of the target is read all the same.
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:00a062bda31894d720907cb0ec23e4bc5dd3e4d0db76200eab3b747b849ca38d
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:2f53d14209715f08aa022f39c528fe655af6686d5dd463d239c591156a170d73
    fixture.write("elfie-compile/maps/rust/a.json", "{\"output\":");
    fixture.write("crates/b/src/b.rs", "// b\n");
    assert_eq!(
        outputs_of(&source_maps_of(&workspace, None)),
        ["crates/b/src/b.rs"]
    );
}

/// The map files of a target's folder are read in the ascending text order of their
/// paths relative to it, so a map file beside a directory of the same name is read
/// where its own path falls rather than after the directory's files.
// @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:e3ab53d40653267f2e160d4a7f3251350dc8120cab5217a982112c74085d6a81
#[test]
fn a_map_file_beside_a_directory_is_read_in_path_order() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("crates/cli.rs", "// cli\n")
        .write("crates/cli/main.rs", "// cli main\n")
        .write("crates/clip.rs", "// clip\n");
    for (map, output, source) in [
        ("cli.json", "crates/cli.rs", "def/cli.lfy"),
        ("cli/main.json", "crates/cli/main.rs", "def/cli/main.lfy"),
        ("clip.json", "crates/clip.rs", "def/clip.lfy"),
    ] {
        write_source_maps(
            &fixture.root.join("elfie-compile/maps/rust").join(map),
            &[map_for(output, source)],
        )
        .unwrap();
    }
    let workspace = load(&fixture.root);
    // `.` sorts before `/`, which sorts before a letter, so `cli.json` comes first,
    // then the file under `cli/`, then `clip.json`.
    assert_eq!(
        outputs_of(&source_maps_of(&workspace, None)),
        ["crates/cli.rs", "crates/cli/main.rs", "crates/clip.rs"]
    );
}

/// A target with no folder under `elfie-compile/maps` is read from `source-map.json`
/// under its output directory instead, and a target with neither has no source maps.
// @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:b78229b17a59641dc63064a2acdaf15c44c023b6546931ffbd8ea03fe24f7a36
#[test]
fn a_target_with_no_folder_falls_back_to_source_map_json() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("crates/a/src/a.rs", "// a\n")
        .write("crates/b/src/b.rs", "// b\n");
    let workspace = load(&fixture.root);
    // No folder under elfie-compile/maps and no legacy file: no source maps.
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:55c87b019cd4d2d7c4a69761ea81c9bd9980381c22407c0e202211f03cfe8dc8
    assert!(source_maps_of(&workspace, None).is_empty());

    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:c968c8acaf4d38cdd8430f58ae5b09a27d1e2e6bd4ee8861bd63a620ddfd7a9c
    write_source_maps(
        &fixture.root.join("src/source-map.json"),
        &[
            map_for("crates/b/src/b.rs", "def/b.lfy"),
            map_for("crates/a/src/a.rs", "def/a.lfy"),
        ],
    )
    .unwrap();
    assert_eq!(
        outputs_of(&source_maps_of(&workspace, None)),
        ["crates/b/src/b.rs", "crates/a/src/a.rs"]
    );

    // Once the folder exists, the legacy file is never read.
    // @lfy def/generation/main.lfy:sourceMapsOf
    write_source_maps(
        &fixture.root.join("elfie-compile/maps/rust/a.json"),
        &[map_for("crates/a/src/a.rs", "def/a.lfy")],
    )
    .unwrap();
    assert_eq!(
        outputs_of(&source_maps_of(&workspace, None)),
        ["crates/a/src/a.rs"]
    );
    // Nothing is written: the legacy file is left where it was, and nothing under
    // elfie-compile/cache is read.
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:1dc22603643b17fad4874110ef09a3391b67d71c28a9f5b35a7dfa18438f3409
    assert!(fixture.root.join("src/source-map.json").is_file());
    assert!(!fixture.root.join("elfie-compile/cache").exists());
    assert!(!fixture.root.join("elfie-compile/maps/rust/b.json").exists());
}

/// The plan, and the unit of `def/a.lfy`, of a project whose `def` holds `a.lfy` and
/// `b.lfy` and whose maps folder holds a map file for each.
fn recorded() -> (Fixture, Program, Plan) {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/a.lfy", "d A { $x = string; }\n")
        .write("def/b.lfy", "d B { $x = string; }\n")
        .write("crates/a/src/a.rs", "// a\n")
        .write("crates/b/src/b.rs", "// b\n");
    write_source_maps(
        &fixture.root.join("elfie-compile/maps/rust/a.json"),
        &[map_for("crates/a/src/a.rs", "def/a.lfy")],
    )
    .unwrap();
    write_source_maps(
        &fixture.root.join("elfie-compile/maps/rust/b.json"),
        &[map_for("crates/b/src/b.rs", "def/b.lfy")],
    )
    .unwrap();
    let program = fixture.program();
    let plan = plan_of(&program, &[], &[], &[]);
    (fixture, program, plan)
}

/// Recording one unit replaces its map file and leaves every other byte for byte as it
/// was, in output order with every object's keys alphabetical.
// @lfy def/generation/main.lfy:record#record:record:4f6f660ecf7fc0b1fd150a14bbc1aade951cb45364ff03577f2d92ae52e33f01
#[test]
fn recording_one_unit_writes_its_map_file_and_leaves_every_other_alone() {
    let (fixture, program, plan) = recorded();
    let workspace = &program.workspace;
    let (_, unit) = unit_of(workspace, &plan, "def/a.lfy");
    let before = fs::read_to_string(fixture.root.join("elfie-compile/maps/rust/b.json")).unwrap();
    let mut map = map_for("crates/a/src/a.rs", "def/a.lfy");
    map.hash = source_hash("a new hash");
    let second = map_for("crates/a/src/other.rs", "def/a.lfy");

    // The unit's map file holds exactly these maps afterwards.
    // @lfy def/generation/main.lfy:record#record:record:d7ed944c0449f01835fd94e7de01e58066347177591b374f6a7beee48113f403
    // @lfy def/generation/main.lfy:record#record:record:5b5e70df30f90156b3023da573235ca899c7a55801fbecd76167819cf57e21ad
    assert!(record(workspace, unit, &[second.clone(), map.clone()]));
    let path = fixture.root.join("elfie-compile/maps/rust/a.json");
    let recorded = read_source_maps(&path);
    // In output order, whatever order they were given in.
    // @lfy def/generation/main.lfy:record
    assert_eq!(
        outputs_of(&recorded),
        ["crates/a/src/a.rs", "crates/a/src/other.rs"]
    );
    assert_eq!(recorded[0].hash, map.hash);
    // No other map file is written.
    // @lfy def/generation/main.lfy:record
    assert_eq!(
        fs::read_to_string(fixture.root.join("elfie-compile/maps/rust/b.json")).unwrap(),
        before
    );
    // The same maps always give the same bytes, and every object's keys are in
    // alphabetical order.
    // @lfy def/generation/main.lfy:record#record:record:5c0c63725b35e7fab925c81abc27914b55863257ede7a2863a3bce15e5b03ff5
    // @lfy def/generation/main.lfy:record#record:record:60d0cd914ebe3c1cbbd582cd3229d8adc3a00b879720f1caeec671fb4c707551
    // @lfy def/generation/main.lfy:record#record:record:1a8586a7f2168dcbbbc34c4373d1f944d6e0313f9b0840a53aa8f54e3e3208d6
    let text = fs::read_to_string(&path).unwrap();
    assert!(record(workspace, unit, &[map.clone(), second]));
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    let keys: Vec<&str> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix('"'))
        .filter_map(|line| line.split_once('"').map(|(key, _)| key))
        .collect();
    assert_eq!(keys.len(), 22, "{text}");
    assert!(
        keys.chunks(11)
            .all(|object| object.windows(2).all(|pair| pair[0] < pair[1])),
        "{text}"
    );
}

/// Maps that differ from what the file holds only in when they were generated leave the
/// file as it is, and no source map at all removes it.
// @lfy def/generation/main.lfy:record#record:record:ad4005f338884cc64145762274daf706be00f7562ff83b81dc47ba0819ebd368
#[test]
fn a_later_generated_time_alone_writes_nothing_and_no_map_removes_the_file() {
    let (fixture, program, plan) = recorded();
    let workspace = &program.workspace;
    let (_, unit) = unit_of(workspace, &plan, "def/a.lfy");
    let path = fixture.root.join("elfie-compile/maps/rust/a.json");
    let before = fs::read_to_string(&path).unwrap();
    let mut map = map_for("crates/a/src/a.rs", "def/a.lfy");
    map.generated = "2030-01-01T00:00:00Z".to_string();
    // Maps that differ only in when they were generated leave the file as it is.
    // @lfy def/generation/main.lfy:record#record:record:acccd867dc43a326a46b14f88ac19bb24373ec1a90795894fa96f075e1fcbcd9
    assert!(record(workspace, unit, std::slice::from_ref(&map)));
    assert_eq!(fs::read_to_string(&path).unwrap(), before);

    // No source map at all removes the file.
    // @lfy def/generation/main.lfy:record#record:record:aa6df37cdc9d7a159ba508148db794c3edbc568247899ccbd55574df85a1177d
    assert!(record(workspace, unit, &[]));
    assert!(!path.exists());
    // Removing what is already gone is recorded all the same.
    assert!(record(workspace, unit, &[]));
    assert!(source_maps_of(workspace, Some("rust")).len() == 1);
}

/// A project cloned without `elfie-compile` records its maps: every missing folder above
/// the map file is created.
// @lfy def/generation/main.lfy:record#record:record:2889ebf117abddc9af4fe7cbf26bd78f7da822afc5bfc70dd4ada149df2c4f17
#[test]
fn recording_creates_every_folder_above_the_map_file() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/cli/main.lfy", "d Cli { $x = string; }\n")
        .write("crates/elfie-cli/src/main.rs", "// main\n");
    let program = fixture.program();
    assert_bound(&program.workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, unit) = unit_of(&program.workspace, &plan, "def/cli/main.lfy");
    assert!(!fixture.root.join("elfie-compile").exists());
    // Every missing folder above the map file is created.
    // @lfy def/generation/main.lfy:record#record:record:1ec300497e4609fbc621214de54b44c8b7f666382b4cf02496efdc55ed4cca99
    assert!(record(
        &program.workspace,
        unit,
        &[map_for("crates/elfie-cli/src/main.rs", "def/cli/main.lfy")]
    ));
    let path = fixture
        .root
        .join("elfie-compile/maps/rust/cli/main.json");
    assert!(path.is_file());
    // The map reads back, so the unit is no longer fresh.
    // @lfy def/generation/main.lfy:sourceMapsOf
    assert_eq!(
        outputs_of(&source_maps_of(&program.workspace, None)),
        ["crates/elfie-cli/src/main.rs"]
    );
}

/// A map file that cannot be written gives `false`.
// @lfy def/generation/main.lfy:record
#[test]
fn a_map_file_that_cannot_be_written_gives_false() {
    let (fixture, program, plan) = recorded();
    let workspace = &program.workspace;
    let (_, unit) = unit_of(workspace, &plan, "def/a.lfy");
    // A directory where the map file belongs cannot be written as a file.
    fs::remove_file(fixture.root.join("elfie-compile/maps/rust/a.json")).unwrap();
    fs::create_dir(fixture.root.join("elfie-compile/maps/rust/a.json")).unwrap();
    // @lfy def/generation/main.lfy:record#record:record:1f4671d2ae5027d4e5bcfaf56dc68c765e27e31a92fc783b7f64a2165f391dd4
    assert!(!record(
        workspace,
        unit,
        &[map_for("crates/a/src/a.rs", "def/a.lfy")]
    ));
}

// -----------------------------------------------------------------------------------
// rederive
// -----------------------------------------------------------------------------------

/// A project whose `def` holds `a.lfy`, declaring `A` with a member `f`, and `b.lfy`,
/// declaring `B`, with the plan and the unit index of `def/a.lfy`.
fn a_and_b_for_rederive(fixture: &Fixture) -> (Program, Plan, usize) {
    fixture
        .write("def/a.lfy", "d A {\n  $f = string;\n}\n")
        .write("def/b.lfy", "d B {\n  $y = string;\n}\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, _) = unit_of(&workspace, &plan, "def/a.lfy");
    (program, plan, index)
}

/// A unit whose regions another unit's split moved into a new file gets one map per file
/// of the output directory that holds a marker of its own, in path order, with the
/// markers each file holds now and everything its last accepted generation recorded
/// carried over; the file it had no map for has no accepted text, so its text is empty.
// @lfy def/generation/main.lfy:rederive#rederive:rederive:8b160ed33ce9dfa87e7f2dc0f80128f847a416833285f4de46a725ff27304afe
#[test]
fn a_split_gives_one_map_per_file_holding_the_units_markers() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write(
            "src/mod.rs",
            "// @lfy def/a.lfy:A\npub struct A;\n// @lfy def/b.lfy:B\npub struct B;\n",
        )
        .write(
            "src/mod/part.rs",
            "// @lfy def/b.lfy:B\nimpl B {}\n// @lfy def/a.lfy:A.f\npub fn f() {}\n",
        );
    let (program, plan, index) = a_and_b_for_rederive(&fixture);
    let recorded = [SourceMap {
        hash: "h".to_string(),
        text: "t".to_string(),
        markers: vec![
            marker_of("def/a.lfy", "A", 1, 2),
            marker_of("def/a.lfy", "A.f", 3, 4),
            marker_of("def/b.lfy", "B", 5, 6),
        ],
        ..map_for("src/mod.rs", "def/a.lfy")
    }];
    let maps = rederive(&program, &plan, index, &recorded);
    // @lfy def/generation/main.lfy:rederive#rederive:rederive:33f6e3ae8b3a61bf5f205818bb8797ab2d0475dc7323d5212b3b38734b4ae594
    assert_eq!(outputs_of(&maps), ["src/mod.rs", "src/mod/part.rs"]);
    // @lfy def/generation/main.lfy:rederive#rederive:rederive:de0c92c8e9054b77050411f99aab03406cef407c814ca97ee7ab8181bdebf960
    assert_eq!(
        maps.iter()
            .map(|map| map
                .markers
                .iter()
                .map(|marker| format!("{}:{}", marker.file, marker.entity.clone().unwrap()))
                .collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        [
            ["def/a.lfy:A", "def/b.lfy:B"],
            ["def/b.lfy:B", "def/a.lfy:A.f"]
        ]
    );
    // @lfy def/generation/main.lfy:rederive#rederive:rederive:17ed6196ab28af701579b3e34153efce5461c44c096f7133d7967b685c4b53c4
    for map in &maps {
        assert_eq!(map.target, "rust");
        assert_eq!(map.source, "def/a.lfy");
        // @lfy def/generation/main.lfy:rederive#rederive:rederive:7656b04818cc881ae3204287ce2ad06c3b43ae1293a39033fad0f9eadb054f2b
        assert_eq!(map.hash, "h");
        assert_eq!(map.generated, recorded[0].generated);
        assert_eq!(map.requirements, recorded[0].requirements);
        assert_eq!(map.signature, recorded[0].signature);
    }
    assert_eq!(maps[0].text, "t");
    // @lfy def/generation/main.lfy:rederive#rederive:rederive:838d2f8ee921d0ec16aab7f04aebafa5071a0f529f8cf1a8a06ed85f0a4e5149
    assert_eq!(maps[1].text, "");
}

/// A shared output another unit added a line to keeps its map: the markers are re-read
/// from the file, while the hash of the text its own acceptance saw is carried over, so
/// the unit's outputs no longer match it.
// @lfy def/generation/main.lfy:rederive#rederive:rederive:ebb597e4a11d98fceeaa2cb175126a053cc1a53ac1ea06a15a083c5036a424f7
#[test]
fn a_shared_output_keeps_its_text_and_re_reads_its_markers() {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "src/lib.rs",
        "// @lfy def/a.lfy:A\npub struct A;\n// @lfy def/b.lfy:B\npub struct B;\n",
    );
    let (program, plan, index) = a_and_b_for_rederive(&fixture);
    let recorded = [SourceMap {
        text: "t".to_string(),
        markers: vec![marker_of("def/a.lfy", "A", 1, 4)],
        ..map_for("src/lib.rs", "def/a.lfy")
    }];
    let maps = rederive(&program, &plan, index, &recorded);
    assert_eq!(outputs_of(&maps), ["src/lib.rs"]);
    assert_eq!(maps[0].text, "t");
    assert_eq!(
        maps[0]
            .markers
            .iter()
            .map(|marker| marker.entity.clone().unwrap())
            .collect::<Vec<_>>(),
        ["A", "B"]
    );
}

/// Where the maps of a unit do not agree on what its last accepted generation recorded,
/// there is nothing to carry over to a file they have no map for, and they come back as
/// they are.
// @lfy def/generation/main.lfy:rederive#rederive:rederive:b7e8748750c50eefa6c4e1df4258cf6235f06854374b6865fe4b5771f2336660
// @lfy def/generation/main.lfy:rederive#rederive:rederive:0ed7b6723ce64cd6cf1e09f24d4516bf399f45d394b5cb0e44753e1df9b7fb01
#[test]
fn maps_that_disagree_on_what_was_recorded_come_back_as_they_are() {
    let fixture = Fixture::with_rust_target();
    fixture.write("src/new.rs", "// @lfy def/a.lfy:A\npub struct A;\n");
    let (program, plan, index) = a_and_b_for_rederive(&fixture);
    let recorded = [
        SourceMap {
            hash: "h1".to_string(),
            ..map_for("src/a.rs", "def/a.lfy")
        },
        SourceMap {
            hash: "h2".to_string(),
            ..map_for("src/b.rs", "def/a.lfy")
        },
    ];
    assert_eq!(rederive(&program, &plan, index, &recorded), recorded);
    // With no map at all, and with no file holding a marker of the unit's own, the maps
    // come back as they are too.
    // @lfy def/generation/main.lfy:rederive#rederive:rederive:ab3fc93517c19e9574351a5b75cd5adfdb9863d5af006fbb2bf0cac32ef94646
    assert!(rederive(&program, &plan, index, &[]).is_empty());
    let (other, _) = unit_of(&program.workspace, &plan, "def/b.lfy");
    let theirs = [map_for("src/b.rs", "def/b.lfy")];
    assert_eq!(rederive(&program, &plan, other, &theirs), theirs);
    // Nothing is written: the file the unit's marker is in is left as it was.
    // @lfy def/generation/main.lfy:rederive#rederive:rederive:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
    assert_eq!(
        fs::read_to_string(fixture.root.join("src/new.rs")).unwrap(),
        "// @lfy def/a.lfy:A\npub struct A;\n"
    );
}

// -----------------------------------------------------------------------------------
// reviewsOf, recordReviews
// -----------------------------------------------------------------------------------

/// A project whose `def` holds `a.lfy`, declaring `A` with two criteria, and `b.lfy`,
/// declaring `B` with one; the plan of its two units comes with the ids of `A`'s.
fn a_with_records() -> (Fixture, Program, Plan, Vec<String>) {
    let fixture = Fixture::with_rust_target();
    fixture
        .write(
            "def/a.lfy",
            "d A: `An a` {\n  @acceptanceCriteria\n    .add({ behavior = `the first` })\n    .add({ behavior = `the second` });\n}\n",
        )
        .write(
            "def/b.lfy",
            "d B: `A b` {\n  $x = string;\n  where (`x is set`) -> `B holds x`;\n}\n",
        );
    let program = fixture.program();
    assert_bound(&program.workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (lowered, entity) = {
        let (_, unit) = unit_of(&program.workspace, &plan, "def/a.lfy");
        (unit.lowered, unit.entities[0])
    };
    let (ids, _) = requirement_ids(&program.files[lowered], entity);
    assert_eq!(ids.len(), 2, "{ids:?}");
    (fixture, program, plan, ids)
}

/// A unit's review record file is `elfie-compile/reviews`, the target's identifier, and
/// the unit's stem with the extension `.json`, under the workspace root — beside its map
/// file and never under it.
// @lfy def/generation/data.lfy:ReviewRecord#ReviewRecord:ReviewRecord:d1283130451aac07da56acfa5179b7af277f4cbbb6d93ed00c6b0aad6499db7a
#[test]
fn a_units_review_record_file_is_under_elfie_compile_reviews_by_target_and_stem() {
    let fixture = Fixture::with_rust_target();
    fixture.write("def/cli/main.lfy", "d Cli { $x = string; }\n");
    let program = fixture.program();
    assert_bound(&program.workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, unit) = unit_of(&program.workspace, &plan, "def/cli/main.lfy");
    assert_eq!(unit.stem, "cli/main");
    assert_eq!(
        review_file(&program.workspace, unit),
        fixture
            .root
            .join("elfie-compile/reviews/rust/cli/main.json")
    );
    assert_ne!(
        review_file(&program.workspace, unit),
        map_file(&program.workspace, unit)
    );
}

/// The records of a unit come back in the order its file lists them; one whose id is of
/// no local criterion or test of the unit is left out, and a file that is no JSON list of
/// records gives none.
// @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:5f98be1ee1d2bca400fb4e2fe3c6433111b63e55a9fd27fe360103362dc8fc62
#[test]
fn the_records_of_a_unit_are_read_in_the_order_the_file_lists_them() {
    let (fixture, program, plan, ids) = a_with_records();
    let (_, unit) = unit_of(&program.workspace, &plan, "def/a.lfy");
    let records = vec![
        ReviewRecord {
            id: ids[1].clone(),
            status: ReviewStatus::Violated,
            hash: source_hash("second"),
        },
        ReviewRecord {
            id: ids[0].clone(),
            status: ReviewStatus::Satisfied,
            hash: source_hash("first"),
        },
    ];
    assert!(record_reviews(&program.workspace, unit, &records));
    // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:d95399b23b29d50a8fba5af56c20731f8f7b0b2bafe16e9914d5090e374ce251
    assert_eq!(reviews_of(&program, unit), records);

    // A record whose id is of no local criterion or test of the unit is left out, so a
    // criterion that was reworded is never carried on its old id's verdict.
    // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:0b41caebdc61220c67f088bc69f465267b2c9e2b36d07c52b02680bf6cb4724c
    let mut foreign = records.clone();
    foreign.push(ReviewRecord {
        id: "A:A:gone".to_string(),
        status: ReviewStatus::Satisfied,
        hash: source_hash("gone"),
    });
    assert!(record_reviews(&program.workspace, unit, &foreign));
    assert_eq!(reviews_of(&program, unit), records);

    // A file that cannot be read as a JSON list of review records gives none.
    // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:0aa4f9eaff20fee7bf263db985597b4d886a2ac762b47b1af1734107a9ac1503
    let path = fixture.root.join("elfie-compile/reviews/rust/a.json");
    fixture.write("elfie-compile/reviews/rust/a.json", "{\"id\":");
    assert!(reviews_of(&program, unit).is_empty());
    fixture.write("elfie-compile/reviews/rust/a.json", "[{\"id\":\"x\"}]");
    assert!(reviews_of(&program, unit).is_empty());

    // Nothing is written, and nothing under elfie-compile/cache is read.
    // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
    // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:1dc22603643b17fad4874110ef09a3391b67d71c28a9f5b35a7dfa18438f3409
    let before = fs::read_to_string(&path).unwrap();
    let _ = reviews_of(&program, unit);
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    assert!(!fixture.root.join("elfie-compile/cache").exists());
}

/// A unit whose review record file was never written has no records, so every criterion
/// and test of it is reviewed again.
// @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:dfd4f71e53b2fb7fce71408d09532e78b71c30b303ef3d8268979cce8c5a7fc3
#[test]
fn a_unit_with_no_review_record_file_has_no_records() {
    let (fixture, program, plan, _ids) = a_with_records();
    let (_, unit) = unit_of(&program.workspace, &plan, "def/a.lfy");
    assert!(!fixture.root.join("elfie-compile/reviews").exists());
    // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:0aa4f9eaff20fee7bf263db985597b4d886a2ac762b47b1af1734107a9ac1503
    assert!(reviews_of(&program, unit).is_empty());
}

/// A project cloned without `elfie-compile` records its verdicts: every missing folder
/// above the review record file is created.
// @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:b4e3424b0ba9d0aee9525f1766d277eebbc0ccc029472634a502abe4447f3c45
#[test]
fn recording_reviews_creates_every_folder_above_the_file() {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/cli/main.lfy",
        "d Cli: `A cli` {\n  $x = string;\n  where (`x is set`) -> `Cli holds x`;\n}\n",
    );
    let program = fixture.program();
    assert_bound(&program.workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (lowered, entity) = {
        let (_, unit) = unit_of(&program.workspace, &plan, "def/cli/main.lfy");
        (unit.lowered, unit.entities[0])
    };
    let (ids, _) = requirement_ids(&program.files[lowered], entity);
    let (_, unit) = unit_of(&program.workspace, &plan, "def/cli/main.lfy");
    assert!(!fixture.root.join("elfie-compile").exists());
    let records = vec![ReviewRecord {
        id: ids[0].clone(),
        status: ReviewStatus::Satisfied,
        hash: source_hash("region"),
    }];
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:72846edcc746900edf9c781c66023ad863b10442a8c62d330bee15766a2090c3
    assert!(record_reviews(&program.workspace, unit, &records));
    let path = fixture
        .root
        .join("elfie-compile/reviews/rust/cli/main.json");
    assert!(path.is_file());
    // The records read back, so the next review carries them.
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:5708abb1bf089eb06657f5688610a6c234b81463164fd199b77b315608316837
    assert_eq!(reviews_of(&program, unit), records);
}

/// Recording one unit's verdicts replaces its review record file and leaves every other
/// byte for byte as it was, in the order given with every object's keys alphabetical; no
/// record at all removes the file, and a file that cannot be written gives `false`.
// @lfy def/generation/main.lfy:recordReviews
#[test]
fn recording_reviews_replaces_the_file_and_leaves_every_other_alone() {
    let (fixture, program, plan, ids) = a_with_records();
    let (_, other) = unit_of(&program.workspace, &plan, "def/b.lfy");
    let (lowered, entity) = (other.lowered, other.entities[0]);
    let (theirs, _) = requirement_ids(&program.files[lowered], entity);
    assert!(record_reviews(
        &program.workspace,
        other,
        &[ReviewRecord {
            id: theirs[0].clone(),
            status: ReviewStatus::Satisfied,
            hash: source_hash("b"),
        }]
    ));
    let beside = fixture.root.join("elfie-compile/reviews/rust/b.json");
    let before = fs::read_to_string(&beside).unwrap();

    let (_, unit) = unit_of(&program.workspace, &plan, "def/a.lfy");
    let records = vec![
        ReviewRecord {
            id: ids[1].clone(),
            status: ReviewStatus::Violated,
            hash: source_hash("second"),
        },
        ReviewRecord {
            id: ids[0].clone(),
            status: ReviewStatus::Satisfied,
            hash: source_hash("first"),
        },
    ];
    assert!(record_reviews(&program.workspace, unit, &records));
    let path = fixture.root.join("elfie-compile/reviews/rust/a.json");
    // The records are listed in the order given, not sorted, and the keys of every
    // object are in alphabetical order.
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:c0e143aef64c3c7bed3e5892dd6d03a0c46df17166ce5ffa598b2db167027369
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:52d12769dcd44be836a5afeb17b9c7341a2a82265bafc029ee658a417f8e0569
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:60d0cd914ebe3c1cbbd582cd3229d8adc3a00b879720f1caeec671fb4c707551
    let text = fs::read_to_string(&path).unwrap();
    let keys: Vec<&str> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix('"'))
        .filter_map(|line| line.split_once('"').map(|(key, _)| key))
        .collect();
    assert_eq!(keys, ["hash", "id", "status", "hash", "id", "status"], "{text}");
    assert!(
        text.find(&ids[1]) < text.find(&ids[0]),
        "the order given: {text}"
    );
    // The same records always give the same bytes, and a second recording replaces what
    // the file held.
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:db1f607aac959c64937cb811664557dedde402eb3fdcaaaf6a2db6b1284ecfd1
    assert!(record_reviews(&program.workspace, unit, &records));
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    assert!(record_reviews(
        &program.workspace,
        unit,
        std::slice::from_ref(&records[0])
    ));
    assert_eq!(reviews_of(&program, unit), [records[0].clone()]);
    // No other unit's file is written.
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:8d039cf20453fa3420e6acdb3ce42b2031909a9bc211b646a683be5017efa7fd
    assert_eq!(fs::read_to_string(&beside).unwrap(), before);

    // No record at all removes the file, and removing what is already gone is recorded
    // all the same.
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:0149e6a1c5272af0b7ca76e627e0fb18e511db4e2076f128db7607e14f49ae85
    assert!(record_reviews(&program.workspace, unit, &[]));
    assert!(!path.exists());
    assert!(record_reviews(&program.workspace, unit, &[]));

    // A directory where the file belongs cannot be written as a file.
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:1f4671d2ae5027d4e5bcfaf56dc68c765e27e31a92fc783b7f64a2165f391dd4
    fs::create_dir(&path).unwrap();
    assert!(!record_reviews(&program.workspace, unit, &records));
}

