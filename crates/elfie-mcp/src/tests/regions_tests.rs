// @lfy def/mcp/main.lfy:output
//! Tests of `output`, `source`, and `search`.

use super::*;
use crate::regions::SEARCHED_LINES;

// @lfy def/mcp/main.lfy:output#output:output:d930026963bc74a22a71de25fff3d4d8ac8a7b00ff6209a9e7d4afbbff550eb1
#[test]
fn output_gives_every_region_of_an_entity_with_its_text() {
    let fixture = accepted_project();
    let session = fixture.session();
    // Each region reads the path, the lines it spans, and the entity the marker names,
    // then the text of those lines in a fenced block.
    // @lfy def/mcp/main.lfy:output#output:output:22ddcfeba7456aa91005e6aeab2fcd871e9c512ede2849961438a5b7a3cc5d7c
    let text = output(&session, "B", None, true).unwrap();
    assert_eq!(text, "out/b.rs:1-2 B\n```rs\n// @lfy def/b.lfy:B\npub struct B;\n```");
    assert_eq!(output(&session, "B", Some("rust"), true).unwrap(), text);
    // The name is the path of an output file, so the regions are every marker it holds.
    // @lfy def/mcp/main.lfy:output#output:output:e3ada846896a5d797c74467acb25cc6ab9c9f3667f13eae813f8169d01373c8f
    // @lfy def/mcp/main.lfy:output#output:output:5f91a9d6a55f8212f9288be65dd547c69391009665dc988e76b8a5150ac20406
    assert_eq!(output(&session, "out/b.rs", None, true).unwrap(), text);
    // Every file under the output directory with an extension of the target's is one, as
    // deep as it lies.
    // @lfy def/mcp/main.lfy:output#output:output:e3ada846896a5d797c74467acb25cc6ab9c9f3667f13eae813f8169d01373c8f
    fixture.write("out/deep/c.rs", "// @lfy def/b.lfy:B\npub struct C;\n");
    let session = fixture.session();
    assert_eq!(output(&session, "out/deep/c.rs", None, false).unwrap(), "out/deep/c.rs:1-2 B");
    // A file below a directory a build or a version control system keeps is no output
    // file, however deep below the output directory it lies, so nothing is generated for
    // its path.
    // @lfy def/mcp/main.lfy:output#output:output:1d6736275de07ac86ca7ade748c7890f146871f150c5b312ffe1d56e39b23e05
    for kept in ["out/target/deep/c.rs", "out/.git/deep/c.rs", "out/a/node_modules/c.rs"] {
        fixture.write(kept, "// @lfy def/b.lfy:B\npub struct C;\n");
        let session = fixture.session();
        assert_eq!(output(&session, kept, None, false).unwrap(), format!("nothing is generated for {kept}"));
    }
    // With `text` false the line of each region stands alone, so however long an output
    // is its regions read as a few lines.
    // @lfy def/mcp/main.lfy:output#output:output:a388e41750cae72a1a126c53b636547055414dad9404ae10cd679aed3861fe6a
    assert_eq!(output(&session, "out/b.rs", None, false).unwrap(), "out/b.rs:1-2 B");
    assert_eq!(output(&session, "B", None, false).unwrap(), "out/b.rs:1-2 B");
    // A call gives `text` as the boolean it is listed as, and it reaches the fn; left
    // out, the parameter's default stands and each region carries its code.
    // @lfy def/mcp/main.lfy:output#output:tool:c74c2a14496848786c2a9b0c1df1da9d6b9205300b34a2f42d670ce562aa0cf3
    let called = call(&session, "elfie_output", serde_json::json!({ "name": "B", "text": false }));
    assert!(!called.is_error, "{}", called.text);
    assert_eq!(called.text, "out/b.rs:1-2 B");
    let called = call(&session, "elfie_output", serde_json::json!({ "name": "B" }));
    assert_eq!(called.text, text);
    // @lfy def/mcp/main.lfy:output#output:tool:76a973ce9145a4809d1d1de1a68875e33067542ee119844cf65ac55c2d7affca
    let called = call(&session, "elfie_output", serde_json::json!({ "name": "B", "text": "no" }));
    assert!(called.is_error && called.text.contains("text"), "{}", called.text);
    // Nothing is generated for the name.
    // @lfy def/mcp/main.lfy:output#output:output:7e2589822812f683a65808151f3ae1177182b076f571c65ab59fa2d7e78162a8
    let none = output(&session, "A", None, true).unwrap();
    assert_eq!(none, "nothing is generated for A");
    // The target is not a known one.
    // @lfy def/mcp/main.lfy:output#output:output:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
    let error = output(&session, "B", Some("go"), true).unwrap_err();
    assert!(error.contains("go is not a target") && error.contains("rust"), "{error}");
}

/// An output file named by its path is read with the markers it holds when the call
/// arrives, not with the ones the last acceptance recorded: `src/a.rs` is nine lines
/// long and holds a marker for `A` on line 1 and one for `b` on line 6, while the
/// recorded map, made before an edit, still puts the second region on line 4.
// @lfy def/mcp/main.lfy:output#output:output:b8c57fc09f7567f3d3edbaa76e9e713c59924bbb396a55209e302c62655135ba
#[test]
fn an_output_file_is_read_with_the_markers_it_holds_now() {
    let fixture = Fixture::new();
    fixture.with_rust_target().write(
        "def/b.lfy",
        "d B {}\nfn b(): `A b` => string { @acceptanceCriteria.add({ behavior = `It answers` }); }\n",
    );
    let local = requirement_id(&fixture, "It answers");
    // The file as the last acceptance saw it: the second region begins on line 4.
    fixture.write(
        "out/b.rs",
        &format!(
            "// @lfy def/b.lfy:B\npub struct B;\n\n// @lfy def/b.lfy:b#{local}\npub fn b() -> String {{\n    String::new()\n}}\n"
        ),
    );
    record_unit(&fixture, "b");
    // The file as it is now: two lines were added before the second marker, so it sits
    // on line 6 and the file is nine lines long.
    fixture.write(
        "out/b.rs",
        &format!(
            "// @lfy def/b.lfy:B\npub struct B;\n\n/// A note.\n/// Another.\n// @lfy def/b.lfy:b#{local}\npub fn b() -> String {{\n    String::new()\n}}\n"
        ),
    );
    let session = fixture.session();
    // @lfy def/mcp/main.lfy:output#output:output:b8c57fc09f7567f3d3edbaa76e9e713c59924bbb396a55209e302c62655135ba
    assert_eq!(
        output(&session, "out/b.rs", None, false).unwrap(),
        format!("out/b.rs:1-5 B\n\nout/b.rs:6-9 b {local}")
    );
    // A marker naming a file that is not in the program gives no region.
    // @lfy def/mcp/main.lfy:output#output:output:84421a06b6eda0b985d11eb52870e3f12014a1594070085b9dc237f580115624
    fixture.write("out/c.rs", "// @lfy def/gone.lfy:C\npub struct C;\n");
    let session = fixture.session();
    assert_eq!(
        output(&session, "out/c.rs", None, false).unwrap(),
        "nothing is generated for out/c.rs"
    );
}

// @lfy def/mcp/main.lfy:output#output:output:8ac14ea384bf2a5e4cb895bab8105f34dfe6124df0ef5ae67edcf3fba77a408d
#[test]
fn output_gives_the_regions_that_answer_for_a_requirement() {
    let fixture = project_with_requirements();
    let local = requirement_id(&fixture, "It answers");
    let global = requirement_id(&fixture, "Nothing is written outside the output directory");
    fixture.write(
        "out/b.rs",
        &format!("// @lfy def/b.lfy:b#{local}\npub fn b() -> String {{\n    String::new()\n}}\n"),
    );
    record_unit(&fixture, "b");
    let session = fixture.session();
    // The text of the criterion comes first, then the region, whose line names the
    // requirement after the entity.
    // @lfy def/mcp/main.lfy:output#output:output:8ac14ea384bf2a5e4cb895bab8105f34dfe6124df0ef5ae67edcf3fba77a408d
    assert_eq!(
        output(&session, &local, None, true).unwrap(),
        format!(
            "It answers\n\nout/b.rs:1-4 b {local}\n```rs\n// @lfy def/b.lfy:b#{local}\npub fn b() -> String {{\n    String::new()\n}}\n```"
        )
    );
    // A global id is read from the lowered program the same way; no region answers for
    // this one, so its text is followed by nothing being generated for it.
    // @lfy def/mcp/main.lfy:output#output:output:8ac14ea384bf2a5e4cb895bab8105f34dfe6124df0ef5ae67edcf3fba77a408d
    assert!(Review::is_global(&global), "{global}");
    let text = output(&session, &global, None, true).unwrap();
    assert_eq!(
        text,
        format!("Nothing is written outside the output directory\n\nnothing is generated for {global}")
    );
}

// The marker read is the one covering the line, and the first line of the text reads
// its file and its line.
// @lfy def/mcp/main.lfy:source#source:source:52459b54250cb2edeb5d2b3079e663f17b6814b8d50602682f49ab2f55bf2285
// @lfy def/mcp/main.lfy:source#source:source:7095bd8049d91a01f9b2a650bf5e71d17bb706c9f4e6a470e004c2dd9881494e
#[test]
fn source_names_the_definition_a_line_came_from() {
    let fixture = accepted_project();
    let session = fixture.session();
    // The line ends with the entity the marker names.
    // @lfy def/mcp/main.lfy:source#source:source:b4c0ce9250c99f427b67858ad3e1b065b76f259ad3bf94c84085ee1551d51ab1
    assert_eq!(source(&session, "out/b.rs", 1).unwrap(), "def/b.lfy:1 B");
    assert_eq!(source(&session, "out/b.rs", 2).unwrap(), "def/b.lfy:1 B");
    // No source map has the file as its output.
    // @lfy def/mcp/main.lfy:source#source:source:3bd573d9905fc8bbb2ff9b436ef7c2affcef119a2a9cf64efa242292f4bf0299
    let text = source(&session, "out/a.rs", 1).unwrap();
    assert_eq!(text, "no source map covers out/a.rs:1");
    // A line past the region of every marker.
    // @lfy def/mcp/main.lfy:source#source:source:3bd573d9905fc8bbb2ff9b436ef7c2affcef119a2a9cf64efa242292f4bf0299
    assert_eq!(source(&session, "out/b.rs", 9).unwrap(), "no source map covers out/b.rs:9");
    // A number argument reaches the fn as a number, and what the fn returns is the
    // call's result.
    // @lfy def/mcp/main.lfy:source#source:tool:e0b14b4706f8dae10a85f002eb93be09aa1f890a3140dca03046fdc0ed52915a
    // @lfy def/mcp/main.lfy:source#source:tool:09d86a7ebefb49c93cedccbd396bfe51c1fd158af3e4aea4f45843eebd4da3a2
    let result = call(&session, "elfie_source", serde_json::json!({ "file": "out/b.rs", "line": 2 }));
    assert!(!result.is_error, "{}", result.text);
    assert_eq!(result.text, "def/b.lfy:1 B");
    // An argument of the wrong type is an error naming it, and the fn is not called.
    // @lfy def/mcp/main.lfy:source#source:tool:6d9dd51635b7204772d74da138f616b326aac0af4536ec450e8f26c7c9650de0
    // @lfy def/mcp/main.lfy:source#source:tool:f64e48f070058897b4b82d2d2649c81b9155f184931f4b71a81c2bb9163b850a
    let result = call(&session, "elfie_source", serde_json::json!({ "file": "out/b.rs", "line": "2" }));
    assert!(result.is_error);
    assert!(result.text.contains("line") && result.text.contains("number"), "{}", result.text);
}

// @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
#[test]
fn source_names_the_requirement_a_region_answers_for() {
    let fixture = project_with_requirements();
    let local = requirement_id(&fixture, "It answers");
    fixture.write(
        "out/b.rs",
        &format!("// @lfy def/b.lfy:b#{local}\npub fn b() -> String {{\n    String::new()\n}}\n"),
    );
    let session = fixture.session();
    assert_eq!(problems(&session, None).unwrap(), "no problems");
    record_unit(&fixture, "b");
    // The marker names a requirement, so its id follows on the line after the one
    // reading the file and the line, and then the text of that criterion.
    // @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
    let session = fixture.session();
    assert_eq!(
        source(&session, "out/b.rs", 2).unwrap(),
        format!("def/b.lfy:1 b\n{local}\nIt answers")
    );
}

/// A region may answer for a global criterion as well as a local one; its text is read
/// from the lowered program the same way.
// @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
#[test]
fn source_reads_a_global_requirement_from_the_lowered_program() {
    let fixture = project_with_requirements();
    let global = requirement_id(&fixture, "Nothing is written outside the output directory");
    assert!(Review::is_global(&global), "{global}");
    fixture.write(
        "out/b.rs",
        &format!("// @lfy def/b.lfy:b#{global}\npub fn b() -> String {{\n    String::new()\n}}\n"),
    );
    record_unit(&fixture, "b");
    let session = fixture.session();
    // @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
    assert_eq!(
        source(&session, "out/b.rs", 1).unwrap(),
        format!("def/b.lfy:1 b\n{global}\nNothing is written outside the output directory")
    );
}

/// `elfie_search` gives each matching line of the outputs with where it is and the
/// region it falls in, and refuses a pattern that is no regular expression.
// @lfy def/mcp/main.lfy:search#search:search:9461946e8b14d460dcf00c7c38a9f31b59d9ef0204bb5dd72c7a8efdf6c5b31a
// @lfy def/mcp/main.lfy:search#search:search:8f67741d05f229bb9234a3e249edc0f82804c4d25ba0235e349aeab582a65ff9
#[test]
fn search_gives_the_lines_of_the_outputs_that_match() {
    let fixture = Fixture::new();
    fixture.with_rust_target().write("def/b.lfy", &two_criteria("It is quick"));
    let first = requirement_id(&fixture, "It answers");
    fixture.write(
        "out/b.rs",
        &format!(
            "// @lfy def/b.lfy:B\npub struct B;\n\n\n\n// @lfy def/b.lfy:b#{first}\npub fn b() -> String {{\nfn helper() {{\n}}\n}}\n"
        ),
    );
    fixture.write("out/c.rs", "pub struct C;\n");
    let session = compiling(&fixture, &[]);
    // The line reads its path, its line, the region between brackets, and its text.
    // @lfy def/mcp/main.lfy:search#search:search:8112c25cc568699c5b153fd7ada26062fb8732b7fcaefef35a49e6389cc0a8ff
    // @lfy def/mcp/main.lfy:search#search:search:fa10d4c06de0e21bae6488a493c889c66e5ea9d22f426fbcf0488df26d15e041
    assert_eq!(
        search(&session, "fn helper[(]", None).unwrap(),
        format!("out/b.rs:8 [def/b.lfy:b#{first}]: fn helper() {{")
    );
    // A line before every marker of its file falls in no region, so its region is a
    // hyphen; the files are given in the order of their paths, then in line order.
    // @lfy def/mcp/main.lfy:search#search:search:b08361d90ea78257dde207e7d32417d0b3458ac1edf1f2c1061ddfb9fcfaa2cd
    // @lfy def/mcp/main.lfy:search#search:search:7841eec414d572a807f6ede13000e8a87220e884b36378adb2feef0da752b8ce
    assert_eq!(
        search(&session, "pub struct", None).unwrap(),
        "out/b.rs:2 [def/b.lfy:B]: pub struct B;\nout/c.rs:1 [-]: pub struct C;"
    );
    // Only the file named, or what is under it, is searched.
    // @lfy def/mcp/main.lfy:search#search:search:6fee06b6e4e8b823d41f3b760f75a448a7c9357ea1409123aaf2b2b62685f6ab
    assert_eq!(
        search(&session, "pub struct", Some("out/c.rs")).unwrap(),
        "out/c.rs:1 [-]: pub struct C;"
    );
    assert_eq!(search(&session, "pub struct", Some("out")).unwrap().lines().count(), 2);
    // @lfy def/mcp/main.lfy:search#search:search:53b64d09870e3da2a2a74debb3f5058f60fda381188b78182eb74ba5aa2269f1
    let error = search(&session, "pub struct", Some("def")).unwrap_err();
    assert!(error.contains("is not under an output directory"), "{error}");
    // No line matches, so the text says so with the count of the files searched.
    // @lfy def/mcp/main.lfy:search#search:search:5fcbe5fc50d6c7ffeea79876a5cf547599a86c0eb99cb0bac1ed79367eee2402
    let none = search(&session, "nowhere", None).unwrap();
    assert!(none.contains("no line of the 2 files searched matches"), "{none}");
    // A pattern that is no regular expression of the Rust regex crate is an error
    // carrying why it is not, and nothing is searched.
    // @lfy def/mcp/main.lfy:search#search:search:8f67741d05f229bb9234a3e249edc0f82804c4d25ba0235e349aeab582a65ff9
    // @lfy def/mcp/main.lfy:search#search:search:b52b7d681a1ab14d16e4367a42f8fe72e5a2c4577ef0e113b5f4e7a1e458ab91
    // @lfy def/mcp/main.lfy:search#search:search:3eb91d30c58a23ec68143fc70f48b65844f317e8c7607db0b879df80b296c435
    let error = search(&session, "fn (", None).unwrap_err();
    assert!(error.contains("is not a regular expression"), "{error}");
}

/// A matching line longer than five hundred characters is cut short, and more than two
/// hundred matching lines are counted rather than all given.
// @lfy def/mcp/main.lfy:search#search:search:68207e69227f6ee9bb5e6b5a46a769482b26d625a8589e28cdcef599d5b1756f
// @lfy def/mcp/main.lfy:search#search:search:a1cf2a0988555448b5857efc090387808f6292fa53e678aeff920abc5253a1b5
#[test]
fn search_cuts_a_long_line_short_and_counts_what_it_leaves_out() {
    let fixture = Fixture::new();
    fixture.with_rust_target().write("def/b.lfy", "d B {}\n");
    let long = "x".repeat(KEPT_WIDTH + 10);
    let mut text = format!("// @lfy def/b.lfy:B\nlet wide = \"{long}\";\n");
    for index in 0..SEARCHED_LINES + 7 {
        text.push_str(&format!("let many{index} = 1;\n"));
    }
    fixture.write("out/b.rs", &text);
    let session = fixture.session();
    // @lfy def/mcp/main.lfy:search#search:search:68207e69227f6ee9bb5e6b5a46a769482b26d625a8589e28cdcef599d5b1756f
    let found = search(&session, "wide", None).unwrap();
    assert!(found.ends_with(CUT), "{found}");
    assert_eq!(found.lines().next().unwrap_or_default().chars().count(), "out/b.rs:2 [def/b.lfy:B]: ".chars().count() + KEPT_WIDTH + 1);
    // @lfy def/mcp/main.lfy:search#search:search:a1cf2a0988555448b5857efc090387808f6292fa53e678aeff920abc5253a1b5
    let many = search(&session, "let many", None).unwrap();
    let lines: Vec<&str> = many.lines().collect();
    assert_eq!(lines.len(), SEARCHED_LINES + 1);
    assert!(lines[SEARCHED_LINES].contains("7 more matching lines are left out"), "{many}");
    assert!(lines[SEARCHED_LINES].contains("narrow the pattern"), "{many}");
}
