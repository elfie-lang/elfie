// @lfy def/generation/main.lfy:plan
//! Tests of markers, hashes, source maps, timestamps, and paths.

use super::*;

// @lfy def/generation/data.lfy:Marker.line
#[test]
fn markers_are_parsed_after_any_line_comment_opener() {
    let text = "// @LFY def/a.lfy:4\nfn x() {}\n# @LFY def/a.lfy:5:2\n-- @LFY def/a.lfy:6 -- more\n; @LFY def/a.lfy:7.\nnothing here\n/* @LFY def/a.lfy:8 */\n// @LFY def/a.lfy:Marker.file\n// @LFY nope\n// @LFY :3\n".replace("@LFY", "@lfy");
    let markers = parse_markers(&text);
    let marker = |output_line, line, column, end| Marker {
        output_line,
        file: "def/a.lfy".to_string(),
        entity: None,
        line,
        column,
        requirement: None,
        end,
    };
    assert_eq!(
        markers,
        [
            marker(1, 4, None, 2),
            marker(3, 5, Some(2), 3),
            marker(4, 6, None, 4),
            marker(5, 7, None, 6),
            marker(7, 8, None, 7),
            // @lfy def/generation/data.lfy:Marker.entity
            Marker {
                output_line: 8,
                file: "def/a.lfy".to_string(),
                entity: Some("Marker.file".to_string()),
                line: 0,
                column: None,
                requirement: None,
                end: 10,
            },
        ]
    );
    assert_eq!(markers[1].spelling(), "@lfy def/a.lfy:5:2");
    assert_eq!(markers[0].spelling(), "@lfy def/a.lfy:4");
    assert_eq!(markers[5].spelling(), "@lfy def/a.lfy:Marker.file");
    assert!(parse_markers("").is_empty());
}

// @lfy def/generation/data.lfy:Marker
#[test]
fn marker_regions_partition_an_output_from_its_first_marker_on() {
    let text =
        "one\n// @LFY def/a.lfy:A\ntwo\nthree\n// @LFY def/a.lfy:B // @LFY def/a.lfy:C\nfour\n"
            .replace("@LFY", "@lfy");
    let markers = parse_markers(&text);
    let regions: Vec<(usize, usize)> = markers
        .iter()
        .map(|marker| (marker.output_line, marker.end))
        .collect();
    // The two markers of line 5 share it: the first ends before it and covers nothing.
    assert_eq!(regions, [(2, 4), (5, 4), (5, 6)]);
    for line in 1..=text.lines().count() {
        let owners = markers.iter().filter(|marker| marker.covers(line)).count();
        assert_eq!(
            owners,
            usize::from(line >= markers[0].output_line),
            "line {line}"
        );
    }
}

// @lfy def/generation/data.lfy:Marker#Marker:Marker:55eeb537f74d15d3b738387a76843bfe4e688258322b57eb5046ead0be1e71dc
#[test]
fn only_a_lfy_path_and_a_line_or_an_identifier_path_is_read_as_a_marker() {
    // Text after `@lfy` that is not a space and a `.lfy` path, a colon, and a number or
    // an identifier path is prose: `@lfydef/a.lfy:4` has no space, and none of these has
    // the rest.
    assert!(parse_markers("// @lfydef/a.lfy:4").is_empty());
    let prose = [
        "@LFY see the notes on markers",
        "@LFY notes.md:4",
        "@LFY README:4",
        "@LFY def/a.rs:4",
        "@LFY .lfy:4",
        "@LFY :4",
        "@LFY def/a.lfy:",
        "@LFY def/a.lfy:4-5",
        "@LFY def/a.lfy:1Marker",
        "@LFY def/a.lfy:Marker..file",
        // A line, with or without a column, spells no requirement, and an empty id is
        // none, so each of these is prose.
        "@LFY def/a.lfy:4#A:A:abc",
        "@LFY def/a.lfy:4:2#A:A:abc",
        "@LFY def/a.lfy:Marker#",
    ];
    for line in prose {
        let text = line.replace("@LFY", "@lfy");
        assert!(parse_markers(&text).is_empty(), "{text}");
    }
    // @lfy def/generation/data.lfy:Marker#Marker:Marker:e5a2dc69b3714f2fe9ec245e19aa4b7d2bc03926cf7410cabc1e0a7ed86cf86c
    let text = "// @LFY def/a.lfy:4\n// @LFY def/deep/a.lfy:Marker\n// @LFY def/a.lfy:_x.y2\n// @LFY def/a.lfy:Marker.file#Marker:Marker:abc\n"
        .replace("@LFY", "@lfy");
    let markers = parse_markers(&text);
    assert_eq!(markers.len(), 4, "{markers:?}");
    assert_eq!(markers[0].line, 4);
    assert_eq!(markers[1].file, "def/deep/a.lfy");
    assert_eq!(markers[1].entity.as_deref(), Some("Marker"));
    assert_eq!(markers[2].entity.as_deref(), Some("_x.y2"));
    assert_eq!(markers[0].requirement, None);
    // A marker read back spells itself as it was written, requirement and all.
    assert_eq!(markers[3].entity.as_deref(), Some("Marker.file"));
    assert_eq!(markers[3].requirement.as_deref(), Some("Marker:Marker:abc"));
    assert_eq!(
        markers[3].spelling(),
        "@lfy def/a.lfy:Marker.file#Marker:Marker:abc"
    );
}

// @lfy def/generation/data.lfy:SourceMap.hash
#[test]
fn the_source_hash_is_lowercase_hex_sha256() {
    assert_eq!(
        source_hash(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        source_hash("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

// @lfy def/generation/data.lfy:SourceMap
#[test]
fn source_maps_round_trip_through_json_and_a_missing_file_gives_none() {
    let fixture = Fixture::new();
    let path = fixture.root.join("src").join("source-map.json");
    assert!(read_source_maps(&path).is_empty());
    let maps = vec![
        SourceMap {
            target: "rust".to_string(),
            output: "src/a.rs".to_string(),
            source: "def/a.lfy".to_string(),
            hash: source_hash("x"),
            requirements: source_hash("the requirements of a"),
            answered: vec!["A:A:abc".to_string()],
            signature: source_hash("interface of a"),
            text: source_hash("the text of src/a.rs"),
            dependencies: BTreeMap::from([(
                "def/b.lfy".to_string(),
                source_hash("interface of b"),
            )]),
            generated: "2026-09-18T10:00:00Z".to_string(),
            markers: vec![
                Marker {
                    output_line: 1,
                    file: "def/a.lfy".to_string(),
                    entity: Some("A".to_string()),
                    line: 4,
                    column: None,
                    requirement: Some("A:A:abc".to_string()),
                    end: 8,
                },
                Marker {
                    output_line: 9,
                    file: "def/a.lfy".to_string(),
                    entity: None,
                    line: 5,
                    column: Some(2),
                    requirement: None,
                    end: 12,
                },
            ],
        },
        SourceMap {
            target: "rust".to_string(),
            output: "src/b.rs".to_string(),
            source: "def/b.lfy".to_string(),
            hash: source_hash("y"),
            requirements: String::new(),
            answered: Vec::new(),
            signature: source_hash("interface of b"),
            text: String::new(),
            dependencies: BTreeMap::new(),
            generated: "2026-09-18T10:00:01Z".to_string(),
            markers: Vec::new(),
        },
    ];
    write_source_maps(&path, &maps).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("[\n"), "{text}");
    assert!(text.contains("\"outputLine\": 9"), "{text}");
    assert_eq!(read_source_maps(&path), maps);
    fixture.write("src/source-map.json", "not json");
    assert!(read_source_maps(&path).is_empty());
    fixture.write("src/source-map.json", "[{\"target\": 1}]");
    assert!(read_source_maps(&path).is_empty());
}

// @lfy def/generation/data.lfy:SourceMap.generated
#[test]
fn timestamps_are_rfc3339_in_utc_to_the_second() {
    assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
    assert_eq!(format_rfc3339(1_789_689_600), "2026-09-18T00:00:00Z");
    assert_eq!(parse_rfc3339("2026-09-18T00:00:00Z"), Some(1_789_689_600));
    assert_eq!(
        parse_rfc3339("2026-09-18T02:00:00.5+02:00"),
        Some(1_789_689_600)
    );
    assert_eq!(
        parse_rfc3339("2026-09-17T22:30:00-01:30"),
        Some(1_789_689_600)
    );
    assert_eq!(parse_rfc3339("yesterday"), None);
    assert_eq!(parse_rfc3339("2026-13-01T00:00:00Z"), None);
    let now = now_rfc3339();
    assert_eq!(now.len(), 20, "{now}");
    assert!(now.ends_with('Z'));
    let seconds = parse_rfc3339(&now).unwrap();
    assert_eq!(format_rfc3339(seconds), now);
    assert!(seconds > 1_789_689_600);
}

// @lfy def/generation/main.lfy:plan
#[test]
fn stems_and_paths() {
    assert!(under("src/a.rs", "src"));
    assert!(under("src/a.rs", ""));
    assert!(!under("srcx/a.rs", "src"));
    assert!(!under("src", "src"));
    assert_eq!(
        order_after_dependencies(&[vec![1], vec![2], vec![]]),
        [2, 1, 0]
    );
    // @lfy def/generation/main.lfy:plan#plan:plan:c6bed32df9850fc2409ccc08a49904114e67a864cad86e4c61b6b0addd1d0dab
    assert_eq!(order_after_dependencies(&[vec![1], vec![0]]), [0, 1]);
    assert_eq!(order_after_dependencies(&[vec![2], vec![0], vec![1]]), [0, 1, 2]);
    // A cycle comes after what any of its items depends on outside it.
    assert_eq!(
        order_after_dependencies(&[vec![1], vec![0, 2], vec![]]),
        [2, 0, 1]
    );
    // @lfy def/generation/main.lfy:plan
    assert_eq!(first_segment("a"), "");
    assert_eq!(first_segment("deep/inner"), "deep");
    assert_eq!(first_segment("deep/deeper/inner"), "deep");
}

/// A stem is the name the file goes by, whatever its path says.
// @lfy def/generation/data.lfy:Unit#Unit:Unit:544d21ab36e8de59ea9fcf77ae7fa34947e690a8e2c4c41f740f290b2efeb354
#[test]
fn a_units_stem_is_the_name_of_its_file() {
    let file = |path: &str, name: &str| File {
        path: path.to_string(),
        package: None,
        name: name.to_string(),
        source: 0,
    };
    assert_eq!(stem_of(&file("def/a.lfy", "a")), "a");
    assert_eq!(stem_of(&file("def/deep/inner.lfy", "deep/inner")), "deep/inner");
    assert_eq!(stem_of(&file("other/x.lfy", "x")), "x");
    assert_eq!(stem_of(&file("vendor/rust/main.lfy", "rust/main")), "rust/main");
}
