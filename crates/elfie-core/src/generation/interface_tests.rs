// @lfy def/generation/main.lfy:interfaceOf
//! Tests of `interfaceOf`.

use super::*;

// @lfy def/generation/main.lfy:interfaceOf#interfaceOf:interfaceOf:df415af10d739698174e3358d1293c5c82973190a3202adf8f50300ab0f4f3d5
#[test]
fn an_interface_holds_one_line_per_entity_with_its_kind_definition_type_and_signature() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
    let text = interface_of(&workspace, b);
    let lines: Vec<&str> = text.lines().collect();
    // One line per entity, and one per member after a data.
    assert_eq!(lines.len(), 3, "{text}");
    assert_eq!(lines[0], "- `B` (data: DataDeclaration): A b — type `B`");
    // @lfy def/generation/main.lfy:interfaceOf
    assert_eq!(lines[1], "  - `x` (member) — type `string`");
    assert!(
        lines[2].starts_with("- `make` (agent function: AgentFunctionDeclaration): Makes a b"),
        "{text}"
    );
    // @lfy def/generation/main.lfy:interfaceOf
    assert!(lines[2].contains("— parameters (x: string)"), "{text}");
    assert!(lines[2].contains("— output `B`"), "{text}");
    // Nothing in an interface is a line number, so nothing moves with the source.
    assert!(!text.contains(":1"), "{text}");
}

// @lfy def/generation/main.lfy:interfaceOf
#[test]
fn a_new_criterion_or_a_moved_line_leaves_the_interface_the_same() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
    let before = interface_of(&workspace, b);

    fixture.write(
        "def/b.lfy",
        &format!(
            "// a comment that moves every line\n{}",
            B.replace(
                "@test(",
                "@acceptanceCriteria.add({ behavior = `a new criterion` });\n  @test(",
            )
        ),
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
    assert_eq!(interface_of(&workspace, b), before);
}

// @lfy def/generation/main.lfy:interfaceOf
#[test]
fn a_member_added_to_a_data_changes_the_interface() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
    let before = interface_of(&workspace, b);

    fixture.write(
        "def/b.lfy",
        &B.replace("$x = string;", "$x = string;\n  $y = number;"),
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
    let after = interface_of(&workspace, b);
    assert_ne!(after, before);
    assert!(
        after.contains("  - `y` (member) — type `number`"),
        "{after}"
    );
}

/// A type line names an entity of the `elfie` package by its identifier alone, such as
/// `List` or `Path`, never by a path or a module name.
// @lfy def/generation/main.lfy:interfaceOf
// @lfy def/generation/data.lfy:SourceMap#SourceMap:SourceMap:38d2eb41999723e89789b375808876aeefd6bae2d41a4f023a97144d33d882a4
#[test]
fn a_type_of_the_elfie_package_is_spelled_by_its_identifier_alone() {
    let fixture = Fixture::with_elfie_package();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
    let text = interface_of(&workspace, a);
    assert!(text.contains("  - `p` (member) — type `Path`"), "{text}");
    assert!(!text.contains("elfie"), "{text}");
    assert!(!text.contains("lib/"), "{text}");
}

/// Every type an interface line spells is spelled by the one speller of a type as Elfie
/// source, so a type written with arguments keeps them and a type that declares no name
/// reads as it was written rather than as the word `anonymous`.
// @lfy def/generation/data.lfy:SourceMap#SourceMap:SourceMap:fd62af8a8ab692d82a9ff1cd315fe1ba7411cb837421fff2038240e570114c59
#[test]
fn an_interface_spells_every_type_as_elfie_source() {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "d Box<T>: `A box` {\n  $item: `What it holds` = T;\n}\n\nd C: `A c` {\n  $boxed: `A box of strings` = Box<string>;\n  $many: `Boxes` = Box<string>[];\n}\n",
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
    let text = interface_of(&workspace, a);
    // A type written with arguments keeps them, so a dependent is told what it holds
    // and a change to the arguments alone changes the signature.
    assert!(
        text.contains("  - `boxed` (member): A box of strings — type `Box<string>`"),
        "{text}"
    );
    assert!(
        text.contains("  - `many` (member): Boxes — type `Box<string>[]`"),
        "{text}"
    );
    // Every type of every line is what the speller gives for it, and nothing the
    // interface spells itself.
    let model = &workspace.model;
    for &entity in &a.entities {
        for member in members_of(model, entity) {
            let Some(spelled) = model::type_text_of(model, model.entities[member].ty.as_ref())
            else {
                continue;
            };
            assert!(
                text.contains(&format!("— type `{spelled}`")),
                "{spelled} is missing:\n{text}"
            );
        }
    }
    assert!(!text.contains("anonymous"), "{text}");
}

