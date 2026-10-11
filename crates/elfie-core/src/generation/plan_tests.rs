// @lfy def/generation/main.lfy:plan
//! Tests of `plan`.

use super::*;

/// The program the plan is for is the workspace lowered: `plan` lowers it itself and
/// gives that program back beside the plan, and every unit names its lowered file by
/// its place in it.
// @lfy def/generation/main.lfy:plan#plan:plan:3303e4532dda74155594c480805a56fc6c02f04473a087e4e299c538bd992142
#[test]
fn the_plan_comes_with_the_workspace_lowered() {
    let fixture = a_and_b();
    let (program, plan) = plan(load(&fixture.root), &[], &[], &[]);
    assert_bound(&program.workspace);
    // What the plan is for is what lowering the same workspace gives.
    // @lfy def/generation/main.lfy:plan
    let lowered = crate::interpret::lower(load(&fixture.root));
    assert_eq!(
        program.files.iter().map(|file| &file.text).collect::<Vec<_>>(),
        lowered.files.iter().map(|file| &file.text).collect::<Vec<_>>()
    );
    assert_eq!(program.files.len(), program.workspace.files.len());
    // Each unit's lowered file is that program's file for the unit's file.
    // @lfy def/generation/main.lfy:plan#plan:plan:f1d41358bbe62b7bc098a9056623218e8f88baf629ec9d4f2caab39e89f37bfc
    assert_eq!(plan.units.len(), 2, "{:?}", plan.units);
    for unit in &plan.units {
        assert_eq!(program.files[unit.lowered].file, unit.file);
    }
}

/// The standard library is compiled by its own project, so none of its entities is
/// ever built, even though `global` holds the target among its targets; a unit's
/// interface may still name them as types.
// @lfy def/generation/main.lfy:plan#plan:plan:0ef10d0db096eac5dd73e22dd85ec6f629b3c05181b46ac57a218cc07d64b221
#[test]
fn the_elfie_package_gives_no_unit_even_though_global_holds_the_target() {
    let fixture = Fixture::with_elfie_package();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    assert!(
        workspace.file("lib/main.lfy").unwrap().package.is_some(),
        "{:?}",
        workspace.files
    );
    let plan = plan_of(&program, &[], &[], &[]);
    assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
    assert_eq!(plan.units[0].file, file_index(&workspace, "def/a.lfy"));
    // Neither `A` nor its file holds a target, so `global`'s targets decide, and every
    // entity the project's own file declares in its file scope is built.
    // @lfy def/generation/main.lfy:plan#plan:plan:d0ff84cddcd136c8da02cfa47f06025000486fd0e764ca59f7baba40be4d9c45
    assert_eq!(names(&workspace, &plan.units[0].entities), ["A"]);
}

// @lfy def/generation/main.lfy:plan#plan:plan:d6ca768a511d5fdd650be5dc4cc490799bb13a803820c6b95c50f95907aa001a
#[test]
fn two_files_give_two_fresh_units_with_b_before_a_in_one_batch() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    assert_eq!(plan.units.len(), 2, "{:?}", plan.units);
    let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
    let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
    // Each unit comes after its dependencies.
    // @lfy def/generation/main.lfy:plan#plan:plan:c6bed32df9850fc2409ccc08a49904114e67a864cad86e4c61b6b0addd1d0dab
    assert!(b_index < a_index);
    // The file `a` uses has a unit of the same target, so that unit is its dependency.
    // @lfy def/generation/main.lfy:plan
    assert_eq!(a.dependencies, [b_index]);
    assert!(b.dependencies.is_empty());
    // Neither unit has an output recorded, so each is fresh.
    // @lfy def/generation/main.lfy:plan#plan:plan:01751bca4d2165839842ab83d64cf2cab13330a74b283b7c2cf83fd8513371c3
    assert_eq!(a.reason, Some(Reason::Fresh));
    assert_eq!(b.reason, Some(Reason::Fresh));
    assert!(a.outputs.is_empty());
    // One unit per file, holding the entities built for the target in file order.
    // @lfy def/generation/main.lfy:plan#plan:plan:913b20182f0e8f00f27782535dbbf5d543af1d94b5560188f87084191178b772
    assert_eq!(names(&workspace, &a.entities), ["A"]);
    assert_eq!(names(&workspace, &b.entities), ["B", "make"]);
    // @lfy def/generation/main.lfy:plan
    assert_eq!(a.stem, "a");
    assert_eq!(b.stem, "b");
    assert_eq!(a.target, 0);
    assert_eq!(a.file, file_index(&workspace, "def/a.lfy"));
    // The batches partition the planned units, in `Plan.units` order.
    // @lfy def/generation/main.lfy:plan#plan:plan:88eb512d83f4888e4edcac53d09e9ed1d190eb09ce99803fb65ec6e80776586c
    assert_eq!(plan.planned().count(), 2);
    assert_eq!(plan.batches[0].units, [b_index, a_index]);
    // One batch, named after the first unit and the count of the others.
    // @lfy def/generation/main.lfy:plan
    assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
    assert_eq!(plan.batches[0].identifier, "b+1");
}

// @lfy def/generation/main.lfy:plan#plan:plan:beb6876df32561e8b00b93be47734a83eab55ef2301d0b18b83bd54cc20f9400
#[test]
fn matching_source_maps_leave_no_reason_and_no_batches() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let maps = current_maps(&program,&plan_of(&program, &[], &[], &[]));
    let plan = plan_of(&program, &maps, &[], &[]);
    assert_eq!(plan.units.len(), 2);
    // Source, requirements, and every dependency's interface all match what the outputs
    // were generated against, so neither unit has a reason.
    // @lfy def/generation/main.lfy:plan#plan:plan:40f45c8240354c40ed6f2a0ef91f2c24787a209455f9f1a93d9e45ae9c1b5dd7
    assert!(
        plan.units.iter().all(|unit| unit.reason.is_none()),
        "{:?}",
        plan.units
    );
    // @lfy def/generation/main.lfy:plan#plan:plan:84b9ddea7d9b6f73bdde004e1e9dee2c746f57ff862c8302fd0a3150a4df7817
    let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
    assert_eq!(a.outputs.len(), 1);
    assert_eq!(a.outputs[0].source, "def/a.lfy");
    assert_eq!(a.outputs[0].target, "rust");
    assert_eq!(plan.planned().count(), 0);
    // Nothing is planned, so there is no batch.
    // @lfy def/generation/main.lfy:plan
    assert!(plan.batches.is_empty());
}

// @lfy def/generation/main.lfy:plan#plan:plan:da57a14227f845b6550a85e82efd1e2eee97963403833ef1c1a4896da8e0a6c6
#[test]
fn a_comment_added_inside_a_dependency_changes_it_and_leaves_its_dependent_alone() {
    let fixture = a_and_b();
    let old = fixture.program();
    assert_bound(&old.workspace);
    let mut maps = current_maps(&old, &plan_of(&old, &[], &[], &[]));

    fixture.write("def/b.lfy", &format!("{B}\n// edited\n"));
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    // The recorded interface signatures still match the current ones.
    let fresh = current_maps(&program,&plan_of(&program, &[], &[], &[]));
    for map in &mut maps {
        let current = fresh.iter().find(|m| m.source == map.source).unwrap();
        assert_eq!(map.signature, current.signature);
        assert_eq!(map.dependencies, current.dependencies);
    }

    let plan = plan_of(&program, &maps, &[], &[]);
    let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
    let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
    // The lowered text differs from what the output was generated from.
    // @lfy def/generation/main.lfy:plan#plan:plan:699a0b8df170aed10dd0c3f2cdf3492c7eff895d33b7e542b8928c357895c458
    assert_eq!(b.reason, Some(Reason::Changed));
    // b is planned only for itself: its interface is the same.
    // @lfy def/generation/main.lfy:plan#plan:plan:40f45c8240354c40ed6f2a0ef91f2c24787a209455f9f1a93d9e45ae9c1b5dd7
    assert_eq!(a.reason, None);
    // @lfy def/generation/main.lfy:plan
    assert_eq!(plan.batches.len(), 1);
    assert_eq!(plan.batches[0].identifier, "b");
}

// @lfy def/generation/main.lfy:plan#plan:plan:b95b8ee170e758c24cb0c08f7daa63a613b0c0d221d5987de5dbfe33dcc489c4
#[test]
fn a_member_added_to_a_dependency_plans_its_dependent_too_in_one_batch() {
    let fixture = a_and_b();
    let old = fixture.program();
    assert_bound(&old.workspace);
    let maps = current_maps(&old, &plan_of(&old, &[], &[], &[]));

    fixture.write(
        "def/b.lfy",
        &B.replace("$x = string;", "$x = string;\n  $y = number;"),
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &maps, &[], &[]);
    let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
    let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
    assert_eq!(b.reason, Some(Reason::Changed));
    // b's interface changed, so what was generated against it is planned again.
    // @lfy def/generation/main.lfy:plan#plan:plan:772d227fa53dc6d543215df30f2905bc04c628366f07dcbff492bb5ce7333638
    assert_eq!(a.reason, Some(Reason::Dependency));
    assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
    assert_eq!(plan.batches[0].units, [b_index, a_index]);
}

/// A target no entity, no file, and not `global` holds among its targets gives no unit.
// @lfy def/generation/main.lfy:plan#plan:plan:cfd8a78a7ab1104d66bbd7d65ce7bb62066a39ae62f6909834bfde20a5a3d84b
#[test]
fn a_target_nothing_names_gives_no_units() {
    let fixture = a_and_b();
    fixture.write("def/targets.lfy", &without_global(TARGETS));
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    assert_eq!(workspace.targets.len(), 1);
    let plan = plan_of(&program, &[], &[], &[]);
    // @lfy def/generation/main.lfy:plan
    assert!(plan.units.is_empty(), "{:?}", plan.units);
    assert!(plan.batches.is_empty());
}

/// An entity whose own targets hold the target is built for it, and an entity of the
/// same file whose own targets are empty is not, because neither its file nor `global`
/// holds the target either.
// @lfy def/generation/main.lfy:plan#plan:plan:d0ff84cddcd136c8da02cfa47f06025000486fd0e764ca59f7baba40be4d9c45
#[test]
fn an_entity_whose_own_targets_hold_the_target_is_built() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/targets.lfy", &without_global(TARGETS))
        .write(
            "def/a.lfy",
            "use \"./targets\";\n\nd Marked {\n  $x = string;\n  @targets.add(rust);\n}\n\
             d Plain { $y = string; }\n",
        );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    let plan = plan_of(&program, &[], &[], &[]);
    assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
    assert_eq!(names(&workspace, &plan.units[0].entities), ["Marked"]);
}

/// The target added to a file's own entity builds everything that file declares in its
/// file scope, and nothing of any other file.
// @lfy def/generation/main.lfy:plan#plan:plan:d0ff84cddcd136c8da02cfa47f06025000486fd0e764ca59f7baba40be4d9c45
#[test]
fn the_target_on_a_files_own_entity_builds_every_entity_of_that_file() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/targets.lfy", &without_global(TARGETS))
        .write(
            "def/a.lfy",
            "use \"./targets\";\n@targets.add(rust);\n\nd One { $x = string; }\n\
             d Two { $y = string; }\n",
        )
        .write("def/bare.lfy", "d Untouched { $x = string; }\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    let plan = plan_of(&program, &[], &[], &[]);
    // One unit, for the file whose own entity holds the target.
    assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
    assert_eq!(
        plan.units[0].file,
        file_index(&workspace, "def/a.lfy"),
        "{:?}",
        plan.units
    );
    assert_eq!(names(&workspace, &plan.units[0].entities), ["One", "Two"]);
    // Only what the file scope declares is built: the members go with the data that
    // holds them.
    // @lfy def/generation/main.lfy:plan#plan:plan:c3b45b9c75fa54e5426018d8de412d9c76d287a296f9384ce03c03717a3ee93c
    assert!(
        !names(&workspace, &plan.units[0].entities)
            .iter()
            .any(|name| name.contains('.')),
        "{:?}",
        plan.units[0].entities
    );
}

/// An entity built for a target whose member reaches an entity of a project file that
/// is not built for it leaves one problem, at that member, naming both and the target.
// @lfy def/generation/main.lfy:plan#plan:plan:28729b42d15d20d9238283f1a0077d5bcf0dffd32f4bf589ea2c0f639e467065
#[test]
fn an_entity_reaching_one_not_built_for_its_target_leaves_a_problem_at_the_member() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/targets.lfy", &without_global(TARGETS))
        .write(
            "def/a.lfy",
            "use \"./targets\";\nuse \"./b\";\n\nd A {\n  $b = B;\n  @targets.add(rust);\n}\n",
        )
        .write("def/b.lfy", "d B { $x = string; }\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    let plan = plan_of(&program, &[], &[], &[]);
    // Only `def/a.lfy` gives a unit, and the `B` its member names is in no unit.
    assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
    assert_eq!(plan.problems.len(), 1, "{:?}", plan.problems);
    let problem = &plan.problems[0];
    for named in ["A.b", "B", "rust"] {
        assert!(problem.message.contains(named), "{problem:?}");
    }
    // It sits at the member whose type reaches it.
    // @lfy def/generation/main.lfy:plan
    let member = workspace
        .model
        .entities
        .iter()
        .position(|entity| {
            entity.node == Some(problem.node) && entity.kind == EntityKind::Member
        })
        .expect("the problem sits at a member");
    assert_eq!(
        workspace.model.entities[member].identifier.as_deref(),
        Some("b")
    );
}

// @lfy def/generation/main.lfy:plan#plan:plan:0ef10d0db096eac5dd73e22dd85ec6f629b3c05181b46ac57a218cc07d64b221
#[test]
fn a_package_file_gives_no_unit_and_a_nested_file_keeps_its_directory_in_its_stem() {
    let fixture = Fixture::with_rust_target();
    fixture.write("def/deep/inner.lfy", "d Inner { $x = string; }\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    // The library declares data `global` holds the target for, but has a package.
    assert!(workspace.file("lib/target/main.lfy").unwrap().package.is_some());
    let plan = plan_of(&program, &[], &[], &[]);
    assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
    // The stem keeps the directory the file was found under.
    // @lfy def/generation/main.lfy:plan
    assert_eq!(plan.units[0].stem, "deep/inner");
    assert_eq!(
        plan.units[0].file,
        file_index(&workspace, "def/deep/inner.lfy")
    );
    assert_eq!(plan.batches[0].identifier, "deep/inner");
}

// @lfy def/generation/main.lfy:plan#plan:plan:c6bed32df9850fc2409ccc08a49904114e67a864cad86e4c61b6b0addd1d0dab
#[test]
fn a_used_file_with_no_unit_contributes_the_units_of_its_own_uses() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write(
            "def/a.lfy",
            "use \"./c\";\nuse \"./b\";\n\nd A { $x = string; }\n",
        )
        .write("def/b.lfy", "d B { $x = string; }\n")
        .write("def/c.lfy", "use \"./d\";\nuse \"./b\";\n")
        .write("def/d.lfy", "d D { $x = string; }\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    assert_eq!(plan.units.len(), 3, "{:?}", plan.units);
    let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
    let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
    let (d_index, _) = unit_of(&workspace, &plan, "def/d.lfy");
    // Each once, in use order: c's uses (d, then b) come through c, then b directly.
    // @lfy def/generation/main.lfy:plan
    assert_eq!(a.dependencies, [d_index, b_index]);
    // @lfy def/generation/main.lfy:plan
    assert!(a_index > b_index && a_index > d_index);
}

// @lfy def/generation/main.lfy:plan#plan:plan:9e6d3d70db8eebce09554eb34d8e0c45134ec1eba5d58f6da4e57b8a378c0f4e
#[test]
fn a_cycle_among_uses_adds_nothing() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/a.lfy", "use \"./b\";\nd A { $x = string; }\n")
        .write("def/b.lfy", "use \"./a\";\nd B { $x = string; }\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    // One problem at each `Use` of the cycle; the plan adds nothing beyond them.
    assert_eq!(workspace.problems.len(), 2, "{:?}", workspace.problems);
    let plan = plan_of(&program, &[], &[], &[]);
    assert_eq!(plan.units.len(), 2);
    let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
    let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
    assert_eq!(a.dependencies, [b_index]);
    assert_eq!(b.dependencies, [a_index]);
    assert_eq!(plan.problems, []);
    assert_eq!(plan.moves, []);
    assert!(
        plan.units
            .iter()
            .all(|unit| unit.reason == Some(Reason::Fresh))
    );
}

/// Two files that use each other and whose entities reach each other too: neither is the
/// lowest file, since each uses the other, so the cycle is the uses' and nothing moves.
// @lfy def/generation/main.lfy:plan#plan:plan:9e6d3d70db8eebce09554eb34d8e0c45134ec1eba5d58f6da4e57b8a378c0f4e
#[test]
fn a_cycle_among_uses_whose_entities_reach_each_other_moves_nothing() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/a.lfy", "use \"./b\";\n\nd A {\n  $b = B;\n}\n")
        .write("def/b.lfy", "use \"./a\";\n\nd B {\n  $a = A;\n}\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    let plan = plan_of(&program, &[], &[], &[]);
    assert_eq!(plan.units.len(), 2, "{:?}", plan.units);
    let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
    let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
    // Each file uses the other, so neither can be the lowest file of the cycle.
    // @lfy def/generation/main.lfy:plan#plan:plan:e32007c54b7c28a4e6102a813e4c61dc2c7f36b487af8e3f5c778220338975cf
    assert!(used_by(&workspace, a.file, b.file));
    assert!(used_by(&workspace, b.file, a.file));
    assert_eq!(lowest_file(&workspace, &[a.file, b.file], &[0, 1]), None);
    // Nothing of the cycle moves, and the plan adds no problem of its own.
    assert_eq!(plan.moves, []);
    assert_eq!(plan.problems, []);
    assert_eq!(names(&workspace, &plan.units[a_index].entities), ["A"]);
    assert_eq!(names(&workspace, &plan.units[b_index].entities), ["B"]);
}

// @lfy def/generation/main.lfy:plan#plan:plan:68e2e70ecc72d488671e852fa7cfa8d7f7c969e753b15735cc176d7a82ed8849
// @lfy def/generation/main.lfy:plan#plan:plan:5ea9c1a46ff002cb966b639dd7fa5752f17660d0d99bf21d939baffb3c345b31
// @lfy def/generation/main.lfy:plan#plan:plan:e32007c54b7c28a4e6102a813e4c61dc2c7f36b487af8e3f5c778220338975cf
// @lfy def/generation/main.lfy:plan#plan:plan:2057c107f5f4608c255f6b82583d495c26804ab224b9335dff27ab7873a8f952
// @lfy def/generation/main.lfy:plan#plan:plan:69e49301647aab00316cdcd47da5c0e5daf7b6857c4849316ed9ed3501ec71bb
#[test]
fn a_cycle_moves_what_the_lowest_file_reaches_into_the_lowest_files_unit() {
    let fixture = a_cycle();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    assert_eq!(plan.units.len(), 2, "{:?}", plan.units);
    let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
    let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");

    // One move, of `C` from the unit of `def/a.lfy` into the unit of `def/b.lfy`, caused
    // by the member the trait added to `B`.
    // @lfy def/generation/main.lfy:plan
    assert_eq!(plan.problems, []);
    assert_eq!(plan.moves.len(), 1, "{:?}", plan.moves);
    let moved = &plan.moves[0];
    assert_eq!(names(&workspace, &moved.entities), ["C"]);
    assert_eq!((moved.origin, moved.into), (a_index, b_index));
    assert_eq!(moved.cause, "B.c");

    // The moved entity is of the unit it moved into and of no other, its own file order
    // first and the moved entity after it.
    // @lfy def/generation/main.lfy:plan#plan:plan:d516ff06fa2ea2c1732f166f5a912d694fba72ac3eede32d49c7ac20123fe48d
    assert_eq!(names(&workspace, &b.entities), ["B", "C"]);
    assert_eq!(names(&workspace, &a.entities), ["A"]);
    // The unit it left depends on the unit it moved into, and the cycle is gone.
    // @lfy def/generation/main.lfy:plan#plan:plan:4a0cb7f3860e04fe587c52b0ce8e9b92e1275445a3ca0b51fd06d8778b47206e
    assert_eq!(a.dependencies, [b_index]);
    assert!(b.dependencies.is_empty(), "{:?}", b.dependencies);
    assert!(b_index < a_index);
    assert!(
        plan.units
            .iter()
            .all(|unit| unit.reason == Some(Reason::Fresh))
    );

    // The text each unit compiles follows its entities: `C` is in the text of the unit it
    // moved into and in no other. @lfy def/generation/main.lfy:plan
    assert!(b.text.contains("d C"), "{}", b.text);
    assert!(!a.text.contains("d C"), "{}", a.text);
    assert!(a.text.contains("d A"), "{}", a.text);
}

/// `def/a.lfy` and `def/b.lfy` reach each other through members a trait of `def/t.lfy`
/// added, and neither uses the other, so no file of the cycle is used by every other.
// @lfy def/generation/main.lfy:plan#plan:plan:b4923bbd4e807e07a830205e8adc5377b41e10ed65314c2ba71fb0cc2ecf3740
// @lfy def/generation/main.lfy:plan#plan:plan:fa7d90dd8bdab380388515f7c40bb7848f1274eb5d75942a04288b75a656c7bf
#[test]
fn a_cycle_with_no_lowest_file_leaves_one_problem_and_moves_nothing() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/a.lfy", "d A {\n  $x = string;\n}\n")
        .write("def/b.lfy", "d B {\n  $x = string;\n}\n")
        .write(
            "def/t.lfy",
            "use \"./a\";\nuse \"./b\";\n\ntrait toB {\n  $b = B;\n}\n\ntrait toA {\n  $a = A;\n}\ntoB.apply(A);\ntoA.apply(B);\n",
        );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
    let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
    // Each reaches the other, and neither file uses the other.
    assert!(a.dependencies.contains(&b_index));
    assert!(b.dependencies.contains(&a_index));
    assert!(!used_by(&workspace, a.file, b.file));
    assert!(!used_by(&workspace, b.file, a.file));

    // Nothing moves, and one problem names every file of the cycle.
    // @lfy def/generation/main.lfy:plan
    assert_eq!(plan.moves, []);
    assert_eq!(names(&workspace, &plan.units[a_index].entities), ["A"]);
    assert_eq!(names(&workspace, &plan.units[b_index].entities), ["B"]);
    assert_eq!(plan.problems.len(), 1, "{:?}", plan.problems);
    let problem = &plan.problems[0];
    assert!(problem.message.contains("def/a.lfy"), "{problem:?}");
    assert!(problem.message.contains("def/b.lfy"), "{problem:?}");
    // It sits at the member of `A` whose type reaches `B`: the first declaration of the
    // cycle, in file order and then source order, that reaches across it.
    // @lfy def/generation/main.lfy:plan
    let member = workspace
        .model
        .entities
        .iter()
        .position(|entity| {
            entity.node == Some(problem.node) && entity.kind == EntityKind::Member
        })
        .expect("the problem sits at a member");
    assert_eq!(
        workspace.model.entities[member].identifier.as_deref(),
        Some("b")
    );
}

/// The problem of a cycle with no lowest file sits at the first declaration that reaches
/// across it, and reaching is transitive: `A.m` is of type `M`, declared in the same file,
/// and only `M` has a member of type `B`, so `A.m` reaches `def/b.lfy` through a type of
/// its own file and is the first such declaration in file order, ahead of the members the
/// trait of `def/t.lfy` added.
// @lfy def/generation/main.lfy:plan#plan:plan:fa7d90dd8bdab380388515f7c40bb7848f1274eb5d75942a04288b75a656c7bf
#[test]
fn a_declaration_reaching_the_other_file_through_its_own_holds_the_cycles_problem() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/a.lfy", "d A {\n  $m = M;\n}\n\nd M {\n  $x = string;\n}\n")
        .write("def/b.lfy", "d B {\n  $x = string;\n}\n")
        .write(
            "def/t.lfy",
            "use \"./a\";\nuse \"./b\";\n\ntrait toB {\n  $b = B;\n}\n\ntrait toA {\n  $a = A;\n}\ntoB.apply(M);\ntoA.apply(B);\n",
        );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
    let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
    // The two units reach each other, and neither file uses the other, so the cycle has
    // no lowest file and nothing moves.
    assert!(a.dependencies.contains(&b_index));
    assert!(b.dependencies.contains(&a_index));
    assert!(!used_by(&workspace, a.file, b.file));
    assert!(!used_by(&workspace, b.file, a.file));
    assert_eq!(plan.moves, []);
    assert_eq!(names(&workspace, &plan.units[a_index].entities), ["A", "M"]);

    assert_eq!(plan.problems.len(), 1, "{:?}", plan.problems);
    let problem = &plan.problems[0];
    assert!(problem.message.contains("def/a.lfy"), "{problem:?}");
    assert!(problem.message.contains("def/b.lfy"), "{problem:?}");
    // `A.m` names no entity of `def/b.lfy`; it is a candidate only because reaching is
    // transitive, and it is the problem's place because `def/a.lfy` comes before the
    // `def/t.lfy` where the members that name `B` and `A` outright are declared.
    // @lfy def/generation/main.lfy:plan
    let member = workspace
        .model
        .entities
        .iter()
        .position(|entity| {
            entity.node == Some(problem.node) && entity.kind == EntityKind::Member
        })
        .expect("the problem sits at a member");
    assert_eq!(
        workspace.model.entities[member].identifier.as_deref(),
        Some("m")
    );
    assert_eq!(
        workspace.files[file_index(&workspace, "def/a.lfy")].source,
        problem.node.file
    );
}

/// Two cycles are resolved in two rounds, each round reading the dependencies the last
/// round left; afterwards no two units of a target reach each other.
// @lfy def/generation/main.lfy:plan#plan:plan:b83fb4e8773d4d2bc8f168802a062c5f8d414beaa418defd0430f736e6928053
#[test]
fn moves_repeat_until_no_cycle_is_left() {
    let fixture = Fixture::with_rust_target();
    for pair in ["1", "2"] {
        fixture
            .write(
                &format!("def/b{pair}.lfy"),
                &format!("d B{pair} {{\n  $x = string;\n}}\n"),
            )
            .write(
                &format!("def/a{pair}.lfy"),
                &format!(
                    "use \"./b{pair}\";\n\nd A{pair} {{\n  $b = B{pair};\n}}\n\nd C{pair} {{\n  $x = string;\n}}\n"
                ),
            );
    }
    fixture.write(
        "def/t.lfy",
        "use \"./a1\";\nuse \"./b1\";\nuse \"./a2\";\nuse \"./b2\";\n\ntrait h1 {\n  $c = C1;\n}\n\ntrait h2 {\n  $c = C2;\n}\nh1.apply(B1);\nh2.apply(B2);\n",
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    assert_eq!(plan.problems, []);
    assert_eq!(plan.moves.len(), 2, "{:?}", plan.moves);
    for pair in ["1", "2"] {
        let (_, b) = unit_of(&workspace, &plan, &format!("def/b{pair}.lfy"));
        let (_, a) = unit_of(&workspace, &plan, &format!("def/a{pair}.lfy"));
        assert_eq!(
            names(&workspace, &b.entities),
            [format!("B{pair}"), format!("C{pair}")]
        );
        assert_eq!(names(&workspace, &a.entities), [format!("A{pair}")]);
    }
    // Nothing is left that depends on something that depends on it.
    // @lfy def/generation/main.lfy:plan
    let dependencies: Vec<Vec<usize>> = plan
        .units
        .iter()
        .map(|unit| unit.dependencies.clone())
        .collect();
    assert_eq!(cycles(&dependencies), Vec::<Vec<usize>>::new());
}

/// Reaching is read from the definitions alone, so an entity built for two targets moves
/// the same way for both.
// @lfy def/generation/main.lfy:plan#plan:plan:2aa0a55e70b64bf9e37087a9332dce7e87a8a1376bb6744dab9b2ce7c058717a
#[test]
fn every_move_is_the_same_for_every_target() {
    let fixture = a_cycle();
    fixture.write(
        "def/targets.lfy",
        &format!(
            "{}\nace const other = targetOf({{ output = \"other\", language = lang, \
             layout = flat }});\nglobal@targets.add(other);\n",
            TARGETS.trim_end()
        ),
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_eq!(workspace.targets.len(), 2, "{:?}", workspace.problems);
    let plan = plan_of(&program, &[], &[], &[]);
    assert_eq!(plan.units.len(), 4, "{:?}", plan.units);
    // One move per target, both of `C` and both into the unit of `def/b.lfy`.
    // @lfy def/generation/main.lfy:plan
    assert_eq!(plan.moves.len(), 2, "{:?}", plan.moves);
    let moved: Vec<(Vec<String>, String, String)> = plan
        .moves
        .iter()
        .map(|moved| {
            (
                names(&workspace, &moved.entities),
                workspace.files[plan.units[moved.into].file].path.clone(),
                moved.cause.clone(),
            )
        })
        .collect();
    assert_eq!(moved[0], moved[1], "{moved:?}");
    assert_eq!(moved[0].1, "def/b.lfy");
    // The cycle is resolved for both, so neither target leaves a problem, and the one
    // cycle is never reported twice. @lfy def/generation/main.lfy:plan
    assert_eq!(plan.problems, []);
}

// @lfy def/generation/main.lfy:plan#plan:plan:68cc4a6ef4fd0e7da9d24e21b1ef05a1e1d173ad7313adf6c3971c21bd01bd3c
// @lfy def/generation/main.lfy:plan#plan:plan:709c271f920bd31e9716cfbd10c64489c0dfa7e2944a2e80c900ba79a40a3d3c
#[test]
fn requested_comes_before_every_other_reason_by_path_or_stem() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let maps = current_maps(&program,&plan_of(&program, &[], &[], &[]));
    let plan = plan_of(&program, &maps, &["b".to_string()], &[]);
    let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
    let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
    assert_eq!(b.reason, Some(Reason::Requested));
    // b's interface did not change, so a is left alone. @lfy def/generation/main.lfy:plan
    assert_eq!(a.reason, None);
    // A path names a unit as well as a stem, and the one requested unit is the one
    // batch, named after its stem alone.
    // @lfy def/generation/main.lfy:plan
    let plan = plan_of(&program, &maps, &["def/a.lfy".to_string()], &[]);
    let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
    let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
    assert_eq!(a.reason, Some(Reason::Requested));
    assert_eq!(b.reason, None);
    assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
    assert_eq!(plan.batches[0].identifier, "a");
}

// @lfy def/generation/main.lfy:plan
#[test]
fn an_output_recording_nothing_for_a_dependency_plans_the_unit() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let mut maps = current_maps(&program,&plan_of(&program, &[], &[], &[]));
    for map in &mut maps {
        map.dependencies.clear();
    }
    let plan = plan_of(&program, &maps, &[], &[]);
    let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
    let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
    assert_eq!(a.reason, Some(Reason::Dependency));
    assert_eq!(b.reason, None);
}

// @lfy def/generation/main.lfy:plan#plan:plan:661840864856b01f6c2f306f700e65aae858a009d8c010124a47b5fd9885985f
#[test]
fn a_batch_holds_at_most_six_units_of_one_directory() {
    let fixture = Fixture::with_rust_target();
    for index in 0..7 {
        fixture.write(
            &format!("def/deep/u{index}.lfy"),
            &format!("d U{index} {{ $x = string; }}\n"),
        );
    }
    fixture.write("def/top.lfy", "d Top { $x = string; }\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    assert_eq!(plan.units.len(), 8, "{:?}", plan.units);
    assert!(
        plan.units
            .iter()
            .all(|unit| unit.reason == Some(Reason::Fresh))
    );
    let sizes: Vec<usize> = plan.batches.iter().map(|batch| batch.units.len()).collect();
    // Six of `deep`, then the seventh, and `top` on its own: no batch holds more than
    // six units, and none mixes directories.
    // @lfy def/generation/main.lfy:plan#plan:plan:46d51e256ac0ed89b3413a7e235770719a0f216b2b18d7b133e7e05e6d930718
    assert_eq!(sizes.iter().sum::<usize>(), 8, "{:?}", plan.batches);
    assert_eq!(sizes, [BATCH_UNITS, 1, 1], "{:?}", plan.batches);
    assert_eq!(plan.batches[0].identifier, "deep/u0+5");
    assert_eq!(plan.batches[1].identifier, "deep/u6");
    assert_eq!(plan.batches[2].identifier, "top");
    // A unit joins the open batch only while it shares the first segment of its stem and
    // the batch holds fewer than six units.
    // @lfy def/generation/main.lfy:plan#plan:plan:f5945b26c42abf21599b311c49e8448eb81d2fad73e72ff70a5affec2d2f63c7
    assert!(sizes.iter().all(|&size| size <= BATCH_UNITS), "{sizes:?}");
    for batch in &plan.batches {
        let first = first_segment(&plan.units[batch.units[0]].stem).to_string();
        for &index in &batch.units {
            assert_eq!(
                first_segment(&plan.units[index].stem),
                first,
                "{:?}",
                plan.batches
            );
        }
    }
    assert_eq!(plan.batches.len(), 3, "{:?}", plan.batches);
}

/// Two units of one directory whose sources together reach the limit go in two batches,
/// though neither the count of units nor the directory would have parted them.
// @lfy def/generation/main.lfy:plan#plan:plan:8b23e12f7a1211640db7cdf6e25859876cd4e9df01a21c0766522b1249acc498
// @lfy def/generation/main.lfy:plan#plan:plan:b5a7643a3fbd7949303b346993c7bec289b1766eb30f651e165467197f2936b0
#[test]
fn two_units_whose_sources_together_reach_the_limit_go_in_two_batches() {
    let fixture = Fixture::with_rust_target();
    // Each file lowers to more than half the limit and less than the whole of it, so
    // either on its own fits a batch and the two together do not.
    let members: String = (0..1_200)
        .map(|index| format!("  $m{index}: `A member of its own name` = string;\n"))
        .collect();
    for name in ["a", "b"] {
        fixture.write(
            &format!("def/x/{name}.lfy"),
            &format!("d {}: `A big one` {{\n{members}}}\n", name.to_uppercase()),
        );
    }
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    assert_eq!(plan.units.len(), 2, "{:?}", plan.units);
    let (_, a) = unit_of(&workspace, &plan, "def/x/a.lfy");
    let (_, b) = unit_of(&workspace, &plan, "def/x/b.lfy");
    for unit in [a, b] {
        let characters = unit.text.chars().count();
        assert!(
            characters < BATCH_CHARACTERS && characters * 2 >= BATCH_CHARACTERS,
            "{characters}"
        );
        assert_eq!(unit.reason, Some(Reason::Fresh));
    }
    // They share the first segment of their stem and would be four units short of the
    // count, so only the size parts them: one batch each, named after its own unit.
    // @lfy def/generation/main.lfy:plan
    assert_eq!(first_segment(&a.stem), first_segment(&b.stem));
    assert_eq!(plan.batches.len(), 2, "{:?}", plan.batches);
    assert_eq!(plan.batches[0].identifier, "x/a");
    assert_eq!(plan.batches[1].identifier, "x/b");
}

// @lfy def/generation/main.lfy:plan#plan:plan:83ae85d53a06f4a097fb8db49ddcb5211746d5285060c427ab68e0f4d46b4b30
// @lfy def/generation/main.lfy:plan#plan:plan:f5f747818d71fc77f0063fbd3976422979d7df380c43c8c14e716c5beb4baa4e
#[test]
fn a_dependent_planned_only_for_its_dependency_joins_its_batch_across_directories() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/core/b.lfy", "d B: `A b` {\n  $x = string;\n}\n")
        .write(
            "def/other/a.lfy",
            "use \"../core/b\";\n\nd A {\n  $b = B;\n}\n",
        );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let maps = current_maps(&program,&plan_of(&program, &[], &[], &[]));

    fixture.write(
        "def/core/b.lfy",
        "d B: `A b` {\n  $x = string;\n  $y = number;\n}\n",
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &maps, &[], &[]);
    let (b_index, b) = unit_of(&workspace, &plan, "def/core/b.lfy");
    let (a_index, a) = unit_of(&workspace, &plan, "def/other/a.lfy");
    assert_eq!(b.reason, Some(Reason::Changed));
    assert_eq!(a.reason, Some(Reason::Dependency));
    assert_ne!(first_segment(&a.stem), first_segment(&b.stem));
    assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
    assert_eq!(plan.batches[0].units, [b_index, a_index]);
    assert_eq!(plan.batches[0].identifier, "core/b+1");
}

/// A request carries one target's guidance and one set of native dependencies, so units
/// of two targets never share a batch however well their stems and sizes would fit.
// @lfy def/generation/main.lfy:plan#plan:plan:139ff97944391c8eb7f3b810897377a8bd64ea54d0fe64b6c29a7043348a5379
#[test]
fn a_unit_of_another_target_opens_a_new_batch() {
    let fixture = a_and_b();
    fixture.write(
        "def/targets.lfy",
        &format!(
            "{}\nace const other = targetOf({{ output = \"other\", language = lang, \
             layout = flat }});\nglobal@targets.add(other);\n",
            TARGETS.trim_end()
        ),
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_eq!(workspace.targets.len(), 2, "{:?}", workspace.problems);
    let plan = plan_of(&program, &[], &[], &[]);
    // Two files for each of the two targets, every one of them fresh.
    assert_eq!(plan.units.len(), 4, "{:?}", plan.units);
    assert!(plan.units.iter().all(|unit| unit.reason == Some(Reason::Fresh)));
    // Both files sit directly under the source directory, so every unit shares the first
    // segment of its stem with every other and only the target parts them: one batch per
    // target, each holding that target's two units.
    assert_eq!(plan.batches.len(), 2, "{:?}", plan.batches);
    for batch in &plan.batches {
        let target = plan.units[batch.units[0]].target;
        assert_eq!(batch.units.len(), 2, "{:?}", plan.batches);
        for &index in &batch.units {
            assert_eq!(plan.units[index].target, target, "{:?}", plan.batches);
        }
    }
    assert_ne!(
        plan.units[plan.batches[0].units[0]].target,
        plan.units[plan.batches[1].units[0]].target
    );
}

// @lfy def/generation/main.lfy:plan#plan:plan:88eb512d83f4888e4edcac53d09e9ed1d190eb09ce99803fb65ec6e80776586c
#[test]
fn every_batch_comes_after_the_batches_holding_its_dependencies() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/one/x.lfy", "d X { $x = string; }\n")
        .write("def/two/y.lfy", "use \"../one/x\";\n\nd Y { $x = X; }\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    assert_eq!(plan.batches.len(), 2, "{:?}", plan.batches);
    let mut seen: Vec<usize> = Vec::new();
    for batch in &plan.batches {
        for &index in &batch.units {
            for &dependency in &plan.units[index].dependencies {
                assert!(
                    seen.contains(&dependency) || batch.units.contains(&dependency),
                    "{:?}",
                    plan.batches
                );
            }
        }
        seen.extend(batch.units.iter().copied());
    }
    assert_eq!(plan.batches[0].identifier, "one/x");
    assert_eq!(plan.batches[1].identifier, "two/y");
}

/// A unit's lowered file is the one lowered from its file, and an entity whose code runs
/// at compile time — a trait, or a declaration inside an `ace` — has no lowered node and
/// is in no unit, so no output has to carry a marker for it.
// @lfy def/generation/main.lfy:plan#plan:plan:913b20182f0e8f00f27782535dbbf5d543af1d94b5560188f87084191178b772
#[test]
fn an_entity_with_no_lowered_node_is_in_no_unit() {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "trait marked: `A trait of its own` {\n}\n\nd A {\n  $x = string;\n}\n\nace d Read {\n  $y = string;\n}\n",
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
    // @lfy def/generation/main.lfy:plan
    assert_eq!(names(&workspace, &unit.entities), ["A"]);
    // @lfy def/generation/main.lfy:plan
    assert_eq!(program.files[unit.lowered].file, unit.file);
    assert_eq!(
        program.files[unit.lowered].text,
        request(&program, &plan, &plan.batches[0], &[], &BTreeMap::new(), false, &[]).sources
            ["def/a.lfy"]
    );
}

/// A criterion added to an entity leaves the lowered code the same, so the unit is
/// planned for its requirements rather than for a change, and its dependent is left
/// alone; the request says the code of such a unit did not change.
// @lfy def/generation/main.lfy:plan#plan:plan:43f3dac0dd4bb25573f9bff0c0f81d674671504e8ac666c8ae2bf37faf2d75f8
#[test]
fn a_new_criterion_plans_the_unit_for_its_requirements() {
    let fixture = a_and_b();
    let old = fixture.program();
    assert_bound(&old.workspace);
    let maps = current_maps(&old, &plan_of(&old, &[], &[], &[]));

    fixture.write(
        "def/b.lfy",
        &B.replace(
            "@test(",
            "@acceptanceCriteria.add({ behavior = `a new criterion` });\n  @test(",
        ),
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &maps, &[], &[]);
    let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
    let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
    // The lowered text is the same; only the ids of the local criteria differ.
    // @lfy def/generation/main.lfy:plan#plan:plan:7725d70d35f6e8be6804e106099b40d9930e162127a1e2c046e3b3a2ec0ff846
    assert_eq!(b.reason, Some(Reason::Requirements));
    assert_eq!(a.reason, None);
    let map = maps.iter().find(|map| map.source == "def/b.lfy").unwrap();
    assert_eq!(map.hash, source_hash(&program.files[b.lowered].text));
    assert_ne!(map.requirements, requirements_hash(&program, b));
    // The unit's own section says its code is unchanged and that its criteria or tests
    // differ. @lfy def/generation/main.lfy:request
    let request = request(&program, &plan, &plan.batches[0], &[], &BTreeMap::new(), false, &[]);
    assert!(
        request
            .instructions
            .contains("The lowered code of this unit is unchanged"),
        "{}",
        request.instructions
    );
}

/// A unit's local criteria and tests are those of each of its entities and of its file's
/// own entity, so moving an entity into another unit moves them with it: the receiving
/// unit holds them and the unit the entity left holds them no longer.
// @lfy def/generation/data.lfy:Unit#Unit:Unit:e5292928d76df9bc73129964e9a39f6ef14fe33c190526dc4bb3f76ff2380810
#[test]
fn a_units_requirements_are_its_entities_and_move_with_them() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    let plan = plan_of(&program, &[], &[], &[]);
    let ids = |unit: &Unit| {
        unit.requirements(&program)
            .iter()
            .map(|requirement| requirement.id().to_string())
            .collect::<Vec<String>>()
    };
    let (a_index, a_unit) = unit_of(&workspace, &plan, "def/a.lfy");
    let (b_index, b_unit) = unit_of(&workspace, &plan, "def/b.lfy");

    // Of the entities of `def/b.lfy`, only `make` carries anything: one criterion and
    // one test. `def/a.lfy` declares `A` alone, which carries neither.
    let b_ids = ids(b_unit);
    assert_eq!(b_ids.len(), 2);
    assert!(
        b_ids.iter().all(|id| id.starts_with("make:make:")),
        "{b_ids:?}"
    );
    assert!(ids(a_unit).is_empty());

    // `make` moved into the unit of `def/a.lfy`, as a cycle among the two files would
    // move it: its criterion and its test are then that unit's, though its declaration
    // is still in the file it came from, and the unit it left holds neither.
    let make = *b_unit
        .entities
        .iter()
        .find(|&&entity| entity_name(&workspace.model, entity) == "make")
        .expect("make is an entity of def/b.lfy");
    let mut moved = plan.clone();
    moved.units[a_index].entities.push(make);
    moved.units[b_index]
        .entities
        .retain(|&entity| entity != make);
    moved.moves.push(Move {
        entities: vec![make],
        origin: b_index,
        into: a_index,
        cause: "A.b".to_string(),
    });
    assert_eq!(ids(&moved.units[a_index]), b_ids);
    assert!(ids(&moved.units[b_index]).is_empty());
}

/// A stem the last global review found violated plans its unit again, and a request
/// still comes first.
// @lfy def/generation/main.lfy:plan#plan:plan:7693e6aca754c3b43b49e3c98a9f0532e6853bec3b10d21a9f22fccc00b021ce
#[test]
fn a_violated_stem_plans_the_unit_again() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let maps = current_maps(&program, &plan_of(&program, &[], &[], &[]));
    let plan = plan_of(&program, &maps, &[], &["b".to_string()]);
    let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
    let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
    // @lfy def/generation/main.lfy:plan#plan:plan:d02d6ed2f21e62878f97bfce1df0638b8152e90e9d1fbd87429ffad5d31a7be5
    assert_eq!(b.reason, Some(Reason::Violated));
    assert_eq!(a.reason, None);
    // A requested stem comes before a violated one.
    // @lfy def/generation/main.lfy:plan
    let plan = plan_of(&program, &maps, &["b".to_string()], &["b".to_string()]);
    let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
    assert_eq!(b.reason, Some(Reason::Requested));
}

/// A global criterion added plans no unit of its own: the global review is run instead,
/// and only a unit whose output answers for one it finds violated is planned again.
// @lfy def/generation/main.lfy:plan
#[test]
fn a_new_global_criterion_plans_no_unit_and_a_violated_one_plans_its_unit() {
    let fixture = a_and_b();
    let old = fixture.program();
    assert_bound(&old.workspace);
    let maps = current_maps(&old, &plan_of(&old, &[], &[], &[]));

    fixture.write(
        "def/b.lfy",
        &format!(
            "{B}\nglobal@acceptanceCriteria.add({{ behavior = `Nothing is written twice` }});\n"
        ),
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    assert_eq!(program.criteria.len(), 1, "{:?}", program.criteria);
    // Nothing is violated, so no unit is planned for the difference.
    // @lfy def/generation/main.lfy:plan#plan:plan:40f45c8240354c40ed6f2a0ef91f2c24787a209455f9f1a93d9e45ae9c1b5dd7
    let plan = plan_of(&program, &maps, &[], &[]);
    assert!(
        plan.units.iter().all(|unit| unit.reason.is_none()),
        "{:?}",
        plan.units
    );
    assert!(plan.batches.is_empty(), "{:?}", plan.batches);
    // The global review found it violated in `b`, so `b` alone is planned again.
    // @lfy def/generation/main.lfy:plan#plan:plan:d02d6ed2f21e62878f97bfce1df0638b8152e90e9d1fbd87429ffad5d31a7be5
    let plan = plan_of(&program, &maps, &[], &["b".to_string()]);
    let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
    let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
    assert_eq!(b.reason, Some(Reason::Violated));
    assert_eq!(a.reason, None);
}

