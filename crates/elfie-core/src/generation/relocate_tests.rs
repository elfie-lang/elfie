// @lfy def/generation/accept.lfy:relocate
//! Tests of `relocate`.

use super::*;

/// A fn `f` with one criterion.
const F_WITH_A_CRITERION: &str =
    "fn f(): `Does f` => string {\n  @acceptanceCriteria.add({ behavior = `it does` });\n}\n";
/// A fn `f` with none.
const F_WITHOUT: &str = "fn f(): `Does f` => string;\n";

/// The project, lowered and planned.
fn planned(fixture: &Fixture) -> (Program, Plan) {
    let program = fixture.program();
    assert_bound(&program.workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    (program, plan)
}

/// The id of the one local criterion of the unit of a file.
fn criterion_of(program: &Program, plan: &Plan, path: &str) -> String {
    let (_, unit) = unit_of(&program.workspace, plan, path);
    unit.requirements(program)[0].id().to_string()
}

/// `src/a.rs` holding `A` under `def/a.lfy`, then `f` under `marked`, once plain and once
/// with the id `id`.
fn a_rs(marked: &str, id: &str) -> String {
    format!(
        "// @lfy def/a.lfy:A\npub struct A;\n// @lfy {marked}:f\npub fn f() -> String {{\n    \
         String::new()\n}}\n// @lfy {marked}:f#{id}\n#[test]\nfn f_does() {{}}\n"
    )
}

// @lfy def/generation/accept.lfy:relocate#relocate:relocate:16d462e16009bd6bf62f282aad018937494a50a8e19adaaa689a406db72a3440
#[test]
fn the_code_of_an_entity_moved_to_another_file_is_relocated_with_its_maps_and_records() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/a.lfy", "d A { $x = string; }\n")
        .write("def/b.lfy", F_WITH_A_CRITERION);
    let (program, plan) = planned(&fixture);
    let workspace = &program.workspace;
    let id = criterion_of(&program, &plan, "def/b.lfy");
    let before = a_rs("def/a.lfy", &id);
    fixture.write("src/a.rs", &before);
    let (a, a_unit) = unit_of(workspace, &plan, "def/a.lfy");
    let recorded = SourceMap {
        answered: vec![id.clone()],
        ..map_for("src/a.rs", "def/a.lfy")
    };
    assert!(record(workspace, a_unit, std::slice::from_ref(&recorded)));
    let own = ReviewRecord {
        id: "A:A:1f2e".to_string(),
        status: ReviewStatus::Satisfied,
        hash: source_hash("A"),
    };
    let of_f = ReviewRecord {
        id: id.clone(),
        status: ReviewStatus::Satisfied,
        hash: source_hash("f"),
    };
    assert!(record_reviews(workspace, a_unit, &[of_f.clone(), own.clone()]));
    let (b, _) = unit_of(workspace, &plan, "def/b.lfy");
    assert!(a < b);

    let relocated = relocate(&program, &plan, None);
    // One relocation of `f`, for rust, from a.lfy into b.lfy, of its two markers.
    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:1a059000fb6ab9058a68deed6e5262648945ef089d52434a0e87147e30c1372f
    assert_eq!(
        relocated.relocations,
        [Relocation {
            target: "rust".to_string(),
            entity: "f".to_string(),
            origin: "def/a.lfy".to_string(),
            into: "def/b.lfy".to_string(),
            markers: 2,
        }]
    );
    // The output with the two markers of `f` naming b.lfy, every other character kept.
    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:06c13663c823f02268ff24aa0f738e83e5f78bd0a30f52df199dc354a1ddeb64
    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:5fb8c262b31de69c1722a9c987d846610497e6d16ff3e10c58ab920056275481
    let after = before
        .replace("// @lfy def/a.lfy:f\n", "// @lfy def/b.lfy:f\n")
        .replace("// @lfy def/a.lfy:f#", "// @lfy def/b.lfy:f#");
    assert_eq!(
        relocated.outputs,
        [Output {
            path: "src/a.rs".to_string(),
            text: after,
        }]
    );
    // a's map and b's, each with the three markers.
    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:84efefcf9edab1fb512d7edbadb3d52ba33e608fead07de81066b06907cd15f5
    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:8792744e69676cd1ef4f2b424f39a10564c50fd60fa42a3724583de37fa00f41
    assert_eq!(relocated.source_maps.len(), 2, "{:?}", relocated.source_maps);
    let of_a = &relocated.source_maps[0];
    let of_b = &relocated.source_maps[1];
    assert_eq!((of_a.source.as_str(), of_a.output.as_str()), ("def/a.lfy", "src/a.rs"));
    assert_eq!((of_b.source.as_str(), of_b.output.as_str()), ("def/b.lfy", "src/a.rs"));
    for map in [of_a, of_b] {
        let files: Vec<&str> = map.markers.iter().map(|marker| marker.file.as_str()).collect();
        assert_eq!(files, ["def/a.lfy", "def/b.lfy", "def/b.lfy"], "{map:?}");
    }
    assert_eq!(of_a.text, recorded.text);
    assert_eq!(of_b.hash, recorded.hash);
    assert_eq!(of_b.requirements, recorded.requirements);
    assert_eq!(of_b.signature, recorded.signature);
    assert_eq!(of_b.dependencies, recorded.dependencies);
    assert_eq!(of_b.generated, recorded.generated);
    assert_eq!(of_b.text, "");
    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:75fa6b15d3932f256be72c37e82f0a66d665d357df32a5fa918c20083d3d798c
    assert_eq!(of_b.answered, std::slice::from_ref(&id));
    // a keeps A's record and b takes f's.
    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:694389998ffd76aa31af05bd6c58accca7643be261e0e0c1c1f5eb06982e3d60
    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:dd567a2eb50a504594a9d8a80a5f42513601274f87909a8fd34ab55432c47b97
    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:fdd1bc51c38f9c62c44b1cbbd6f35402ebc120996898c5ea02ed62ade39fe519
    assert_eq!(
        relocated.reviews,
        [
            UnitReviews {
                target: "rust".to_string(),
                stem: "a".to_string(),
                records: vec![own],
            },
            UnitReviews {
                target: "rust".to_string(),
                stem: "b".to_string(),
                records: vec![of_f],
            },
        ]
    );
    // Nothing is written.
    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
    assert_eq!(fs::read_to_string(fixture.root.join("src/a.rs")).unwrap(), before);
    assert!(!review_file(workspace, &plan.units[b]).exists());
}

/// A unit whose map names a rewritten output, though no file holds a marker of its own, keeps
/// its maps as read, as rederive gives them.
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:84efefcf9edab1fb512d7edbadb3d52ba33e608fead07de81066b06907cd15f5
#[test]
fn a_unit_whose_map_names_a_rewritten_output_keeps_its_maps_as_rederive_gives_them() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/a.lfy", "d A { $x = string; }\n")
        .write("def/b.lfy", F_WITH_A_CRITERION)
        .write("def/c.lfy", "d C { $x = string; }\n");
    let (program, plan) = planned(&fixture);
    let workspace = &program.workspace;
    let id = criterion_of(&program, &plan, "def/b.lfy");
    fixture.write("src/a.rs", &a_rs("def/a.lfy", &id));
    let (_, a_unit) = unit_of(workspace, &plan, "def/a.lfy");
    let (_, c_unit) = unit_of(workspace, &plan, "def/c.lfy");
    assert!(record(workspace, a_unit, &[map_for("src/a.rs", "def/a.lfy")]));
    let of_c = map_for("src/a.rs", "def/c.lfy");
    assert!(record(workspace, c_unit, std::slice::from_ref(&of_c)));
    let relocated = relocate(&program, &plan, None);
    assert_eq!(relocated.relocations.len(), 1, "{:?}", relocated.relocations);
    let kept: Vec<&SourceMap> = relocated
        .source_maps
        .iter()
        .filter(|map| map.source == "def/c.lfy")
        .collect();
    assert_eq!(kept, [&of_c]);
}

/// Two files declare `f` now, and only one of them answers for the id its code carries.
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:6efd150e921c3acb701bf44dea80a76db4af961969869f78eeb54165cd5e08a3
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:a519caccbc677cf75a549ad61efab1ac3308d321a0569fe1405c1b60e878a212
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:235508d273bf7a0baa2e362b069972c22f8112165ad4c2446e0c3a312196c918
#[test]
fn only_the_file_that_still_answers_for_the_code_takes_it() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/a.lfy", "d A { $x = string; }\n")
        .write("def/b.lfy", F_WITHOUT)
        .write("def/c.lfy", F_WITH_A_CRITERION);
    let (program, plan) = planned(&fixture);
    let id = criterion_of(&program, &plan, "def/c.lfy");
    fixture.write("src/a.rs", &a_rs("def/a.lfy", &id));
    let relocated = relocate(&program, &plan, None);
    assert_eq!(relocated.relocations.len(), 1, "{:?}", relocated.relocations);
    assert_eq!(relocated.relocations[0].into, "def/c.lfy");
    assert_eq!(relocated.relocations[0].markers, 2);
}

/// Two files declare `f` now, and nothing tells them apart, so nothing moves.
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:aefaa6e860114ee4208ffcf0030ae946f4913f0378f704d07d57e7d418ffd430
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:be73db54a2d75fbbad58de702fb271bc2d14eff7a087d844a030c8318ee2a2eb
#[test]
fn an_entity_two_files_declare_alike_is_not_relocated() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/a.lfy", "d A { $x = string; }\n")
        .write("def/b.lfy", F_WITHOUT)
        .write("def/c.lfy", F_WITHOUT);
    let (program, plan) = planned(&fixture);
    let workspace = &program.workspace;
    fixture.write("src/a.rs", "// @lfy def/a.lfy:A\npub struct A;\n// @lfy def/a.lfy:f\npub fn f() {}\n");
    let (_, a_unit) = unit_of(workspace, &plan, "def/a.lfy");
    assert!(record(workspace, a_unit, &[map_for("src/a.rs", "def/a.lfy")]));
    let relocated = relocate(&program, &plan, None);
    assert!(relocated.relocations.is_empty(), "{:?}", relocated.relocations);
    assert!(relocated.outputs.is_empty());
    assert!(relocated.reviews.is_empty());
    assert_eq!(relocated.source_maps, source_maps_of(workspace, None));
}

/// Every marker names an entity its file declares, and an output under a directory named
/// target is never read.
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:2e2e5b2ed5da7292f36fa19f46de7380162949db3677787156e0796eaf9cc0c9
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:ed503738db6fcf03f63af23719e2dac5524eb316ff0a2527f41665d52e24ba05
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:4d0103d3f96ff9071e2501adbecf2688ff28d6f0b3a3450a4eaa87de84415d55
// @lfy def/generation/accept.lfy:relocate#relocate:relocate:d571fb77bdde93f51e6308b17cf2d32fc4f3abd9a421a361b314697542ddd5f9
#[test]
fn nothing_is_relocated_while_every_marker_names_what_its_file_declares() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/a.lfy", "d A { $x = string; }\n")
        .write("def/b.lfy", F_WITHOUT);
    let (program, plan) = planned(&fixture);
    let workspace = &program.workspace;
    fixture
        .write("src/a.rs", "// @lfy def/a.lfy:A\npub struct A;\n")
        .write("src/b.rs", "// @lfy def/b.lfy:f\npub fn f() {}\n")
        .write("src/target/old.rs", "// @lfy def/a.lfy:f\npub fn f() {}\n");
    let (_, a_unit) = unit_of(workspace, &plan, "def/a.lfy");
    assert!(record(workspace, a_unit, &[map_for("src/a.rs", "def/a.lfy")]));
    let relocated = relocate(&program, &plan, None);
    assert!(relocated.relocations.is_empty(), "{:?}", relocated.relocations);
    assert!(relocated.outputs.is_empty());
    assert!(relocated.reviews.is_empty());
    // @lfy def/generation/accept.lfy:relocate#relocate:relocate:b365192b3311058a64e4835640b7d8f4d411459fd0cebd5be19c821e4b3e6288
    assert_eq!(relocated.source_maps, source_maps_of(workspace, None));
    assert_eq!(relocated.source_maps.len(), 1);
}
