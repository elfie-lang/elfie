// @lfy def/generation/main.lfy:request
//! Tests of `request`.

use super::*;

// @lfy def/generation/main.lfy:request#request:request:b4934a86310e0664a47f5c551266b5e7905fd31e14c52b40314353cca9b1b0f8
// @lfy def/generation/main.lfy:request#request:request:571bc891f6509a3d1997b846f685a316e34804763d8271373888f530426558b3
// @lfy def/generation/main.lfy:request#request:request:800766e448bfb8ed73777735dcb6d1a81a50d15783302ad27486b92c2d669f5e
#[test]
fn a_request_names_the_target_lists_its_units_and_quotes_their_entities() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
    let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
    let batch = &plan.batches[0];
    let request = request(&program, &plan,batch, &[], &BTreeMap::new(), false, &[]);
    // @lfy def/generation/main.lfy:request#request:request:f3979f1497ff9e1b60be372fddce29ffcfe830689ef46680b690554afd9c7a37
    assert_eq!(request.batch, *batch);
    assert_eq!(request.batch.units, [b_index, a_index]);
    // The sources are the lowered text of each unit's file: the body of a fn is its
    // criteria and tests, which are read rather than generated, so what is left of one
    // is its signature.
    // @lfy def/generation/main.lfy:request
    let (_, b_unit) = unit_of(&workspace, &plan, "def/b.lfy");
    assert_eq!(
        request.sources["def/b.lfy"],
        program.files[b_unit.lowered].text
    );
    assert!(
        request.sources["def/b.lfy"].contains("fn make(x: string): `Makes a b` => B;"),
        "{}",
        request.sources["def/b.lfy"]
    );
    assert!(
        !request.sources["def/b.lfy"].contains("@acceptanceCriteria"),
        "{}",
        request.sources["def/b.lfy"]
    );
    assert!(request.sources["def/a.lfy"].contains("d A: `An a` {"));
    // The requirements hold every local criterion and test of the file, each with its
    // id: one criterion and one test, both of `make`.
    // @lfy def/generation/main.lfy:request
    assert_eq!(request.requirements["def/b.lfy"].len(), 2);
    assert!(
        request.requirements["def/b.lfy"]
            .iter()
            .all(|requirement| requirement.id().starts_with("make:make:")),
        "{:?}",
        request.requirements["def/b.lfy"]
    );
    // Nothing of this project is added to global, so there is no global requirement.
    // @lfy def/generation/main.lfy:request
    assert!(request.globals.is_empty(), "{:?}", request.globals);
    assert!(request.previous.is_empty());
    // @lfy def/generation/main.lfy:request
    assert!(request.existing.is_empty());
    // Both units are in the batch, so nothing is an interface.
    // @lfy def/generation/main.lfy:request
    assert!(request.interfaces.is_empty());
    // The declaration has no criterion of its own, the layout and the ecosystem none
    // either, and the language one; `target` is all that they extend, and it has none.
    // @lfy def/generation/main.lfy:request
    assert_eq!(
        request.guidance,
        guidance_for(&workspace.model, &workspace.targets[0])
    );
    assert_eq!(request.guidance.len(), 1);
    // What the target's own dependencies, commands, and knowledge give.
    // @lfy def/generation/main.lfy:request#request:request:3d9e3582187fd1f1532a46bf0f938c787311c81feb59f4a7d7d5b0da9a813a9a
    assert_eq!(
        request
            .native_dependencies
            .iter()
            .map(|dependency| dependency.identifier.as_str())
            .collect::<Vec<_>>(),
        ["serde_json", "sha2"]
    );
    assert_eq!(request.native_dependencies, workspace.targets[0].native_dependencies);
    assert_eq!(request.commands, workspace.targets[0].commands);
    assert!(request.knowledge.is_empty(), "{:?}", request.knowledge);

    let text = &request.instructions;
    assert!(text.contains("compiler for the target `rust`"), "{text}");
    assert!(text.contains("- `b` (`def/b.lfy`)"), "{text}");
    assert!(text.contains("- `a` (`def/a.lfy`)"), "{text}");
    assert!(text.contains("under `src`"), "{text}");
    assert!(text.contains("### `def/a.lfy` (stem `a`)"), "{text}");
    assert!(text.contains("#### `A` (data: DataDeclaration)"), "{text}");
    // The entity's lowered code, not its source: the documentation and the body a
    // declaration is read for are gone.
    // @lfy def/generation/main.lfy:request
    assert!(text.contains("d A: `An a` {\n  $b = B;\n}"), "{text}");
    assert!(text.contains("Definition: An a"), "{text}");
    assert!(
        text.contains("Each unit becomes one module named after its stem"),
        "{text}"
    );
    assert!(text.contains("`serde_json` (cargo), version `1`"), "{text}");
    assert!(text.contains("`sha2` (cargo), any version"), "{text}");
    assert!(
        text.contains("The units of this batch depend on no unit outside it."),
        "{text}"
    );
    // Markers name entities, never lines, and sit in a line comment beginning with the
    // target's marker comment.
    // @lfy def/generation/main.lfy:request#request:request:671617e25d98e36f8c646b0c76d941034839245f2544e239cb4ede3581bb2f33
    assert!(text.contains("Never write a line number."), "{text}");
    assert!(
        text.contains("a line comment beginning with `//`, then `@lfy`"),
        "{text}"
    );
    assert!(text.contains("`// @lfy def/b.lfy:B`"), "{text}");
    // The three report lines.
    // @lfy def/generation/main.lfy:request
    assert!(text.contains("`ELFIE: DONE`"), "{text}");
    assert!(text.contains("`ELFIE: BLOCKED: `"), "{text}");
    assert!(text.contains("`ELFIE: CLARIFY: `"), "{text}");
    // The tools of the agent server are named, and are called before files are read by
    // hand.
    // @lfy def/generation/main.lfy:request
    for tool in [
        "before files are searched or read by hand",
        "`elfie_entity`",
        "`elfie_find`",
        "`elfie_references`",
        "`elfie_outline`",
        "`elfie_output`",
        "`elfie_source`",
        "`elfie_changes`",
        "`elfie_check`",
    ] {
        assert!(text.contains(tool), "{tool} is missing:\n{text}");
    }
    // The standard library is bound, never generated.
    // @lfy def/generation/main.lfy:request
    assert!(
        text.contains("carry `builtin` are bound to what the guidance"),
        "{text}"
    );
    assert!(
        text.contains("written out in full is translated where it is used"),
        "{text}"
    );
    assert!(!text.contains("## Existing outputs"), "{text}");
    // A criterion is never restated in an output, only named by its id.
    // @lfy def/generation/main.lfy:request
    assert!(
        text.contains("Never restate a criterion or a test in an output"),
        "{text}"
    );

    // The commands run with `sh -c` in the output directory, and a dependency is added
    // only by running the add command.
    // @lfy def/generation/main.lfy:request#request:request:09b0190ee8969e01b9ff7a561a98c3663f87fb8b97fa83c588d0631e7989bc20
    assert!(text.contains("These run with `sh -c` in `src`"), "{text}");
    assert!(text.contains("`RUN_SCRIPT`"), "{text}");
    assert!(
        text.contains("`NAME` and `VERSION` set to the dependency's"),
        "{text}"
    );
    assert!(
        text.contains("never by editing a manifest or a lockfile by hand"),
        "{text}"
    );
    assert!(
        text.contains("The build, test, and lint commands are run before the report"),
        "{text}"
    );
    assert!(
        text.contains("The compiler is given nothing of its own to read."),
        "{text}"
    );

    // The sections come in the definition's order.
    // @lfy def/generation/main.lfy:request#request:request:d650f0548150afb04ee79524e308a0150e9cf8a3f4dcc9ffde74e3d90e7fcada
    let headings = [
        "# Compiling",
        "## Where the outputs go",
        "## Guidance",
        "## Native dependencies",
        "## Knowledge",
        "## Interfaces",
        "## Commands",
        "## The units to compile",
        "## Global criteria and tests",
        "## Criteria and tests in an output",
        "## The standard library",
        "## Rules for kinds",
        "## Markers",
        "## Tests",
        "## The agent server",
        "## How to report",
    ];
    let positions: Vec<usize> = headings
        .iter()
        .map(|heading| {
            text.find(heading)
                .unwrap_or_else(|| panic!("{heading} is missing:\n{text}"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{positions:?}"
    );
}

// @lfy def/generation/main.lfy:request#request:request:d86c17626a07f5e8ec807bd7740a5f6f8280e6af67444bf1bf5d18744b801d3b
// @lfy def/generation/main.lfy:request#request:request:72801863f22ee154bf0e22299d1405c97cd6309fc630197aa315508e2f4829f7
#[test]
fn a_request_quotes_every_criterion_and_test_of_an_entity() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
    let batch = Batch {
        units: vec![b_index],
        identifier: "b".to_string(),
    };
    let request = request(&program, &plan, &batch, &[], &BTreeMap::new(), false, &[]);
    let text = &request.instructions;
    assert!(
        text.contains("#### `make` (agent function: AgentFunctionDeclaration)"),
        "{text}"
    );
    // Each criterion and each test is on its own line, with its id.
    // @lfy def/generation/main.lfy:request
    let (criteria, tests) = {
        let (_, unit) = unit_of(&workspace, &plan, "def/b.lfy");
        let entity = *unit.entities.last().expect("make is an entity of b");
        requirement_ids(&program.files[unit.lowered], entity)
    };
    assert!(
        text.contains(&format!(
            "- `{}` — When x is empty: the b holds x",
            criteria[0]
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "- `{}` — Input `[\"y\"]` gives `B@like(`holding y`)`",
            tests[0]
        )),
        "{text}"
    );
}

/// Every global criterion and test of the program is in one section of its own, each
/// once with its id, and the instructions say what a unit does with one.
// @lfy def/generation/main.lfy:request
#[test]
fn a_request_holds_every_global_criterion_once_with_its_id() {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "d A { $x = string; }\nglobal@acceptanceCriteria.add({ behavior = `Nothing is written twice` });\n",
    );
    let program = fixture.program();
    assert_bound(&program.workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let request = request(&program, &plan, &plan.batches[0], &[], &BTreeMap::new(), false, &[]);
    // @lfy def/generation/main.lfy:request
    assert_eq!(
        request
            .globals
            .iter()
            .map(|requirement| requirement.id())
            .collect::<Vec<_>>(),
        [program.criteria[0].id.as_str()]
    );
    let text = &request.instructions;
    assert!(text.contains("## Global criteria and tests"), "{text}");
    assert!(
        text.contains(&format!(
            "- `{}` — Nothing is written twice",
            program.criteria[0].id
        )),
        "{text}"
    );
    // @lfy def/generation/main.lfy:request
    assert!(text.contains("These hold across the whole program"), "{text}");
    assert!(
        text.contains("the global review checks each one once"),
        "{text}"
    );
    // It is not in the unit's own section: a local criterion and a global one are kept
    // apart.
    // @lfy def/generation/main.lfy:request
    assert_eq!(text.matches(program.criteria[0].id.as_str()).count(), 1, "{text}");
}

/// A target whose declaration is given a criterion by an add call at the top of its
/// file, whose layout layer and whose language layer each hold criteria, whose layout
/// layer extends `targetFramework` too and fills the frameworks slot as well, and whose
/// language is applied with an argument: the guidance is what no layer and no trait a
/// layer extends gave, then each layer's in guidance order, then those of every trait
/// the layers extend, nearest first and each trait once, with every template value
/// rendered from the layer's arguments.
// @lfy def/generation/main.lfy:request#request:request:da24d79558458ac142aa1e4f6b149223ba5ddfe55377ebb46b2bf0a367536d5f
#[test]
fn the_guidance_is_the_declarations_criteria_then_its_layers_then_what_they_extend() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write(
            "lib/target/main.lfy",
            &LIBRARY_TARGET
                .replace(
                    ".outputExtensions = [];",
                    ".outputExtensions = [];\n  \
                     where (`An output is written`) -> `Its path is under the output directory`;",
                )
                .replace(
                    "$fileExtension: `The extension of its files` = string;",
                    "$fileExtension: `The extension of its files` = string;\n  \
                     where (`A d declaration is built`) -> `It becomes a type`;",
                )
                .replace(
                    "trait targetLayout extends target: `Where its files go` { }",
                    "trait targetLayout extends target: `Where its files go` {\n  \
                     where (`A unit is written`) -> `Its output is under the output directory`;\n}",
                ),
        )
        .write(
            "def/roles.lfy",
            "trait rust(edition: string) extends targetLanguage: `Rust` {\n  \
             .fileExtension = \"rs\";\n  .markerComment = \"//\";\n  \
             where (`A unit is written`) -> `Each unit becomes one module named after its stem`;\n  \
             where (`An output is written`) -> `Outputs are built as Rust {{edition}}`;\n}\n\
             trait flat extends targetLayout, targetFramework: `One file per unit` {\n  \
             where (`A unit is written`) -> `Its output is its stem and the extension`;\n}\n\
             trait built extends targetFramework: `Built on something, silently` { }\n",
        )
        .write(
            "def/targets.lfy",
            "use \"elfie/target\";\nuse \"./roles\";\n\n\
             ace const t = targetOf({ output = \"src\", language = rust.layer(\"2024\"), \
             frameworks = [flat, built], layout = flat });\n\
             t@acceptanceCriteria.add({ situation = `An output is written`, \
             behavior = `Nothing outside the output directory is written` });\n\
             global@targets.add(t);\n",
        )
        .write("def/a.lfy", "d A { $x = string; }\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    // `flat` fills the frameworks slot and the layout slot, so it is one layer, at the
    // first of the two in guidance order: before `built`, which fills the frameworks slot
    // after it. Were `flat` the layout alone, `built` would come first.
    assert_eq!(
        names(&workspace, &workspace.targets[0].layers),
        ["flat", "built", "rust"]
    );
    let plan = plan_of(&program, &[], &[], &[]);
    assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
    let request = request(&program, &plan, &plan.batches[0], &[], &BTreeMap::new(), false, &[]);
    let behaviors: Vec<String> = request
        .guidance
        .iter()
        .map(|criterion| criterion.behavior.clone().unwrap_or_default().join(" "))
        .collect();
    // What the file's add call gave first, since no layer and no trait a layer extends
    // gave it; then the layout and the language, in guidance order, with the language's
    // template value rendered from its argument; then the traits they extend, nearest
    // first.
    // @lfy def/generation/main.lfy:request
    assert_eq!(
        behaviors,
        [
            "Nothing outside the output directory is written",
            "Its output is its stem and the extension",
            "Each unit becomes one module named after its stem",
            "Outputs are built as Rust 2024",
            "Its output is under the output directory",
            "It becomes a type",
            "Its path is under the output directory",
        ],
        "{:?}",
        request.guidance
    );
    // `target` is reached through `targetLayout` and through `targetLanguage`, and
    // contributes once, at its first place.
    // @lfy def/generation/main.lfy:request
    assert_eq!(
        behaviors
            .iter()
            .filter(|behavior| *behavior == "Its path is under the output directory")
            .count(),
        1,
        "{behaviors:?}"
    );
    // `flat` fills two slots, and contributes once as well.
    // @lfy def/generation/main.lfy:request
    assert_eq!(
        behaviors
            .iter()
            .filter(|behavior| *behavior == "Its output is its stem and the extension")
            .count(),
        1,
        "{behaviors:?}"
    );
    // Every criterion is quoted in the instructions, in that order.
    let text = &request.instructions;
    let positions: Vec<usize> = behaviors
        .iter()
        .map(|behavior| {
            text.find(behavior.as_str())
                .unwrap_or_else(|| panic!("{behavior} is missing:\n{text}"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{positions:?}"
    );
    // A layer applied with arguments has its template values rendered from them, so no
    // template is left in the guidance.
    // @lfy def/generation/main.lfy:request
    assert!(!text.contains("{{edition}}"), "{text}");
}

/// The knowledge of the target, of the batch's entities, and of the entities of an
/// interface, each item once: a reference quoted in full, one named to be read, a
/// definition as the files its use path resolves to, and a tool of the agent server.
// @lfy def/generation/main.lfy:request#request:request:3d9e3582187fd1f1532a46bf0f938c787311c81feb59f4a7d7d5b0da9a813a9a
#[test]
fn the_knowledge_is_quoted_named_resolved_or_called_by_its_kind() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("docs/style.md", "Two spaces, never a tab.\n")
        .write("docs/shape.md", "A long guide nobody quotes.\n")
        .write(
            "def/a.lfy",
            "use \"./b\";\n\nd A {\n  $b = B;\n  @knowledge.add({ topic = `the house style`, \
             kind = KnowledgeKind.reference, source = \"docs/style.md\", quote = true });\n  \
             @knowledge.add({ topic = `the shape of a module`, kind = KnowledgeKind.example, \
             source = \"docs/shape.md\" });\n  @knowledge.add({ topic = `what a b is`, \
             kind = KnowledgeKind.definition, source = \"./b\" });\n  \
             @knowledge.add({ topic = `the program`, kind = KnowledgeKind.tool, \
             source = \"elfie_entity\" });\n}\n",
        )
        .write("def/b.lfy", "d B { $x = string; }\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
    let batch = Batch {
        units: vec![a_index],
        identifier: "a".to_string(),
    };
    let request = request(&program, &plan, &batch, &[], &BTreeMap::new(), false, &[]);
    // Each item once, in the order the request gathers them.
    // @lfy def/generation/main.lfy:request
    assert_eq!(
        request
            .knowledge
            .iter()
            .map(|item| item.topic.as_str())
            .collect::<Vec<_>>(),
        [
            "the house style",
            "the shape of a module",
            "what a b is",
            "the program"
        ]
    );
    let text = &request.instructions;
    // A reference whose quote is true is quoted in full, from its source joined to the
    // root of the project its contributor is declared in.
    // @lfy def/generation/main.lfy:request#request:request:f64fa963c69a9e30aa2c190c595acdbb6221274d050a5a8533b9cf9291684155
    assert!(text.contains("From `docs/style.md`:"), "{text}");
    assert!(text.contains("Two spaces, never a tab."), "{text}");
    // One whose quote is left out is named, to be read first.
    // @lfy def/generation/main.lfy:request#request:request:737e4198f2f6bddc790512ae099b13648ed59405ba19506fb0abff20748ec530
    assert!(
        text.contains(
            "- the shape of a module: read `docs/shape.md` before writing anything it covers."
        ),
        "{text}"
    );
    assert!(!text.contains("A long guide nobody quotes."), "{text}");
    // A definition names the files its use path resolves to, read as Elfie.
    // @lfy def/generation/main.lfy:request#request:request:179394ea2be57b4a87b41dac2988b30f36b058129a701fc79ff34cc290769606
    assert!(
        text.contains("- what a b is: read `def/b.lfy` as Elfie."),
        "{text}"
    );
    // A tool is called before anything relies on what it answers.
    // @lfy def/generation/main.lfy:request#request:request:18fa49f6ae0fe059f4d8b65429f7356e822363b0bb901d611f266ca1393c940b
    assert!(
        text.contains(
            "- the program: call the tool `elfie_entity` of the agent server before relying on \
             what it answers."
        ),
        "{text}"
    );
}

// @lfy def/generation/main.lfy:request#request:request:1530126fe56400a7920d4c5c53eb10a8785a3ed3921d00bc2814a0452615fd57
// @lfy def/generation/main.lfy:request#request:request:7424b0fcb62916f23bfb6c1265f3a3f943fb1354558e0d4b0ea4722212bc0c7f
#[test]
fn a_dependency_outside_the_batch_is_an_interface_with_its_outputs_and_entities() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let maps = current_maps(&program,&plan_of(&program, &[], &[], &[]));
    let plan = plan_of(&program, &maps, &["a".to_string()], &[]);
    let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
    let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
    assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
    assert_eq!(plan.batches[0].units, [a_index]);
    let request = request(&program, &plan,&plan.batches[0], &[], &BTreeMap::new(), false, &[]);
    assert_eq!(request.interfaces.len(), 1);
    assert_eq!(request.interfaces[0].unit, b_index);
    assert_eq!(request.interfaces[0].outputs, ["src/b.rs"]);
    assert_eq!(
        names(&workspace, &request.interfaces[0].entities),
        ["B", "make"]
    );
    let text = &request.instructions;
    assert!(text.contains("### `def/b.lfy`"), "{text}");
    assert!(
        text.contains("- `B` (data: DataDeclaration): A b"),
        "{text}"
    );
    assert!(text.contains("  - `x` (member) — type `string`"), "{text}");
    assert!(
        text.contains("- `make` (agent function: AgentFunctionDeclaration): Makes a b"),
        "{text}"
    );
    assert!(text.contains("never edit their outputs"), "{text}");
}

/// A request for a hosted compiler session names each interface on one line — the file,
/// its outputs, and its entities — and says to read one with `elfie_dependency`, while
/// the request still holds every entity of the dependency; its tools part names
/// `elfie_task` and `elfie_mark`, and its report part `elfie_submit` and the three lines.
// @lfy def/generation/main.lfy:request#request:request:5dac7a621007b979c24fe5cf0995510a2642dd47d72fb40a852433feb2a68360
#[test]
fn a_hosted_request_names_its_interfaces_and_the_tools_of_its_session() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let maps = current_maps(&program, &plan_of(&program, &[], &[], &[]));
    let plan = plan_of(&program, &maps, &["a".to_string()], &[]);
    let request = request(&program, &plan, &plan.batches[0], &[], &BTreeMap::new(), true, &[]);
    // Hosted or not, the request holds every entity of the dependency.
    // @lfy def/generation/main.lfy:request#request:request:5eb8b7bd8df8b28d5af1e781560a38611e0b664872e17a14d9c2e7352a7798cf
    assert_eq!(
        names(&workspace, &request.interfaces[0].entities),
        ["B", "make"]
    );
    let text = &request.instructions;
    // One line naming the file, its outputs, and its entities.
    // @lfy def/generation/main.lfy:request#request:request:c5448d7c2bb4f497fbdd859f9d7405cd0bdf319c54990d943d33d646b6da87d4
    assert!(
        text.contains("- `def/b.lfy`: outputs `src/b.rs`; entities `B`, `make`"),
        "{text}"
    );
    // No definition, kind, or type of an entity of the interface is spelled out, and
    // the section a request quotes them in is not there at all.
    assert!(!text.contains("### `def/b.lfy`"), "{text}");
    assert!(!text.contains("(data: DataDeclaration): A b"), "{text}");
    // @lfy def/generation/main.lfy:request#request:request:91c07c652cfc9edd8b82e5c1bebc5ef3998ae8e528cbd1623161418170399772
    assert!(text.contains("with `elfie_dependency`"), "{text}");
    // @lfy def/generation/main.lfy:request#request:request:d7b94bd7ef8593cddf59f563c5936407b8173a96ce3ad639afb1cdab7cd6f327
    for part in [
        "`elfie_task` gives the obligations of this batch again",
        "marked new, kept, or unknown",
        "`elfie_mark` writes a marker line",
        "never type an id by hand",
    ] {
        assert!(text.contains(part), "{part} is missing:\n{text}");
    }
    // @lfy def/generation/main.lfy:request#request:request:4a237f69a0cf92b366172dde838e3583135a0dc294c8ef43f2392d08c1c1e1bd
    assert!(
        text.contains("Submit the outcome through `elfie_submit` before your final message"),
        "{text}"
    );
    for line in ["`ELFIE: DONE`", "`ELFIE: BLOCKED: `", "`ELFIE: CLARIFY: `"] {
        assert!(text.contains(line), "{line} is missing:\n{text}");
    }

    // A request that is not hosted quotes the interface whole and names none of those
    // tools.
    // @lfy def/generation/main.lfy:request#request:request:1530126fe56400a7920d4c5c53eb10a8785a3ed3921d00bc2814a0452615fd57
    let text =
        super::request(&program, &plan, &plan.batches[0], &[], &BTreeMap::new(), false, &[])
            .instructions;
    assert!(text.contains("### `def/b.lfy`"), "{text}");
    assert!(text.contains("- `B` (data: DataDeclaration): A b"), "{text}");
    assert!(!text.contains("elfie_dependency"), "{text}");
    assert!(!text.contains("elfie_task"), "{text}");
    assert!(!text.contains("elfie_mark"), "{text}");
    assert!(!text.contains("elfie_submit"), "{text}");
}

/// The commands part says which operations the agent server runs through a tool of its
/// own, and which variables narrow such a run.
// @lfy def/generation/main.lfy:request#request:request:f700bbb14b11b81fadfe6c18b018250b1c7b9cdadbd0bd8b2a66ce896dfc54c9
// @lfy def/generation/main.lfy:request#request:request:f4b1430a5e2784e149775d2e69bdf4d652782d426521aa999f67d64b6c8a76c5
#[test]
fn the_commands_part_says_which_tool_runs_an_operation_and_what_narrows_it() {
    let fixture = a_and_b();
    let program = fixture.program();
    assert_bound(&program.workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let text = request(&program, &plan, &plan.batches[0], &[], &BTreeMap::new(), false, &[])
        .instructions;
    for part in [
        "Where the agent server offers `elfie_build`, `elfie_test`, `elfie_lint`, or \
         `elfie_format`, the command of that operation is run through that tool.",
        "Run a line by hand only for an operation whose tool is not offered.",
        "The filter of `elfie_test` is given to the test command as `TESTS`",
        "the scope of `elfie_build`, `elfie_test`, and `elfie_lint` is given to the command \
         as `SCOPE`",
        "narrows the run only where the command's line reads it",
    ] {
        assert!(text.contains(part), "{part} is missing:\n{text}");
    }
    // The tools part names every tool of the agent server a compiler reads the program
    // and its outputs through.
    // @lfy def/generation/main.lfy:request#request:request:43974a3eb5968199e4c0d2b5b84afd218c864eb58c47c072bbcc471866a03921
    for part in [
        "`elfie_entity` for a definition with its criteria and tests",
        "`elfie_find` for where a name is declared",
        "`elfie_references` for where it is used",
        "`elfie_outline` for the declarations of a file",
        "with `text` false, for the regions of an output file without their code",
        "`elfie_search` for the lines of the outputs that match a pattern",
        "`elfie_source` for where a line of generated code came from",
        "`elfie_changes` for what changed in a unit since its outputs were accepted",
        "`elfie_check` for whether acceptance would pass",
    ] {
        assert!(text.contains(part), "{part} is missing:\n{text}");
    }
}

/// The section of a unit entities moved into says which they are, from which file, and
/// because of which cause, and the section of the unit they left says which they are and
/// which unit compiles them.
// @lfy def/generation/main.lfy:request#request:request:78a8534645507634a61935dcee1699d73fdc5735bcadda06eb24bf9f9854ea34
// @lfy def/generation/main.lfy:request#request:request:d8077ccebe94afe4cb2e7e3cf3daafbef0270bf0ca0b6141d42e3be46d002c52
#[test]
fn the_section_of_a_unit_says_what_moved_into_it_and_out_of_it() {
    let fixture = a_cycle();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
    let request = request(&program, &plan, &plan.batches[0], &[], &BTreeMap::new(), false, &[]);
    let text = &request.instructions;

    // In the section of `def/b.lfy`, which `C` moved into.
    // @lfy def/generation/main.lfy:request
    let into = text
        .find("### `def/b.lfy`")
        .expect("a section for def/b.lfy");
    let out = text
        .find("### `def/a.lfy`")
        .expect("a section for def/a.lfy");
    assert!(into < out, "{text}");
    let moved_in = text
        .find("- `C` from `def/a.lfy`, because of `B.c`.")
        .unwrap_or_else(|| panic!("what moved into def/b.lfy's unit: {text}"));
    assert!((into..out).contains(&moved_in), "{text}");
    assert!(
        text.contains("markers naming the file that declares them"),
        "{text}"
    );
    assert!(
        text.contains("never by importing that file"),
        "{text}"
    );

    // In the section of `def/a.lfy`, which `C` moved out of.
    // @lfy def/generation/main.lfy:request
    let moved_out = text
        .find("- `C` in the unit `b` (`def/b.lfy`).")
        .unwrap_or_else(|| panic!("what moved out of def/a.lfy's unit: {text}"));
    assert!(moved_out > out, "{text}");
    assert!(text.contains("never write them again"), "{text}");
    assert!(
        text.contains("under the name and path it would have had"),
        "{text}"
    );

    // The source of each unit is the text it compiles, so `C` is in the source of the
    // unit it moved into and in no other. @lfy def/generation/main.lfy:request
    assert!(request.sources["def/b.lfy"].contains("d C"), "{text}");
    assert!(!request.sources["def/a.lfy"].contains("d C"), "{text}");
}

/// An existing output is kept only where its path is among the outputs of a unit of the
/// batch, and the previous source each was generated from is the file as it was, lowered
/// the way the unit's own source is, so that the two texts can be read side by side.
// @lfy def/generation/main.lfy:request#request:request:ce520ffba1e8298b5ff2f6a6376e60e59a17343e640d10ed179732cd788b8075
#[test]
fn existing_outputs_are_kept_only_where_they_are_among_the_units_outputs() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let maps = current_maps(&program,&plan_of(&program, &[], &[], &[]));
    let plan = plan_of(&program, &maps, &["a".to_string()], &[]);
    let existing = [
        Output {
            path: "src/a.rs".to_string(),
            text: "// old".to_string(),
        },
        Output {
            path: "src/other.rs".to_string(),
            text: "// other".to_string(),
        },
    ];
    // The text as written, which the request lowers itself.
    let previous = BTreeMap::from([(
        "def/a.lfy".to_string(),
        "use \"./b\";\n\nd A: `An a` {\n  $b = B;\n}\n".to_string(),
    )]);
    let with_previous = request(&program, &plan,&plan.batches[0], &existing, &previous, false, &[]);
    assert_eq!(with_previous.existing.len(), 1);
    assert_eq!(with_previous.existing["src/a.rs"], "// old");
    // `Request.previous` is the lowered text of the file as it was, never the text as
    // written: it is read against `Request.sources`, which is lowered too.
    // @lfy def/generation/main.lfy:request#request:request:6232735fed99654f7d25efa96687134e92c7667e0be166ba2471ca9d018c3103
    assert_eq!(with_previous.previous.keys().collect::<Vec<_>>(), ["def/a.lfy"]);
    let was = &with_previous.previous["def/a.lfy"];
    assert_ne!(was, &with_previous.sources["def/a.lfy"]);
    // It is what lowering the file as it was gives: here the same file but for the
    // documentation comment the current one carries.
    // @lfy def/generation/main.lfy:request#request:request:e6a9a7f8f68b31456fa6f45f2971df1eddc6eed03b9bcb155872fff58c0ca957
    let lowered = crate::interpret::lower(crate::workspace::change(
        &workspace,
        "def/a.lfy",
        Some(&previous["def/a.lfy"]),
    ));
    let index = lowered
        .workspace
        .files
        .iter()
        .position(|file| file.path == "def/a.lfy")
        .unwrap();
    assert_eq!(was, &lowered.files[index].text);
    assert!(!was.contains("A record."), "{was}");
    assert!(with_previous.sources["def/a.lfy"].contains("A record."));
    let text = &with_previous.instructions;
    // Only what the difference, the marked criteria and tests, and a split the guidance
    // requires call for changes; names and structure are otherwise kept, and the markers
    // of unchanged items are kept. The outputs part says so, not a part of its own.
    // @lfy def/generation/main.lfy:request#request:request:ce2766fa22ba976093fabd91c70450cb8ddf39a9d8bd6e5084875bc916c84425
    assert!(
        text.contains("Keep the markers of unchanged items."),
        "{text}"
    );
    assert!(
        text.contains(
            "Keep names and structure, except that a module the target's guidance says to \
             split gains submodule files and has its code moved into them: that split is the \
             one change of structure allowed, and it keeps every name other units use."
        ),
        "{text}"
    );
    assert!(
        text.contains("the criteria and tests marked new, reworded, or removed"),
        "{text}"
    );
    assert!(
        text.contains("and a split the target's guidance requires, require."),
        "{text}"
    );
    let said = text
        .find("These outputs exist already:")
        .unwrap_or_else(|| panic!("{text}"));
    assert!(said < text.find("## Guidance").unwrap(), "{text}");
    assert!(said > text.find("## Where the outputs go").unwrap(), "{text}");
    // The outputs of a unit that has some go at the paths its `Unit.outputs` names, except
    // that a module the guidance says to split gains submodule files.
    // @lfy def/generation/main.lfy:request#request:request:08f6ffbf337200835b96675249633150fae7cb0d93486e08ccd3b5c9f63847c7
    assert!(
        text.contains(
            "write to the same paths, except that a module the target's guidance says to \
             split gains submodule files and has its code moved into them."
        ),
        "{text}"
    );
    assert!(text.contains("- `src/a.rs`"), "{text}");
    assert!(text.contains(was.trim_end()), "{text}");

    // With no previous source, the existing outputs of that unit are reconciled with it.
    // @lfy def/generation/main.lfy:request#request:request:3c1f4cba72285105fde5be93378fbda2d3d93aaf4293fbe643ab3a97d7d1b779
    let without = request(
        &program,
        &plan,
        &plan.batches[0],
        &existing,
        &BTreeMap::new(),
        false,
        &[],
    );
    assert!(
        without
            .instructions
            .contains("Reconcile the existing \noutputs of each of them with its source:")
            || without
                .instructions
                .contains("Reconcile the existing outputs of each of them with its source:"),
        "{}",
        without.instructions
    );
}

/// A unit whose outputs recorded no answered ids leaves the request nothing to say which
/// of its criteria and tests are new, so the outputs part says to read the existing code
/// against each of them.
// @lfy def/generation/main.lfy:request#request:request:a960df7e08bfeb90c2fb135fc71e6e80e01a69f9f351d3d8c7eab08f069407c1
#[test]
fn a_unit_whose_outputs_answered_nothing_is_read_against_every_criterion() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let mut maps = current_maps(&program, &plan_of(&program, &[], &[], &[]));
    for map in &mut maps {
        map.answered.clear();
    }
    let plan = plan_of(&program, &maps, &["b".to_string()], &[]);
    let existing = [Output {
        path: "src/b.rs".to_string(),
        text: "// old".to_string(),
    }];
    let request = request(&program, &plan, &plan.batches[0], &existing, &BTreeMap::new(), false, &[]);
    assert!(request.answered.is_empty(), "{:?}", request.answered);
    assert!(
        request
            .instructions
            .contains("Which criteria and tests of these units are new is not known."),
        "{}",
        request.instructions
    );
    assert!(
        request.instructions.contains("- `def/b.lfy`"),
        "{}",
        request.instructions
    );
}

/// A unit whose lowered code is unchanged but whose criteria differ: its section says so,
/// each criterion the outputs did not answer for is marked new, and each one the unit no
/// longer has is listed under a heading of its own with its text.
// @lfy def/generation/main.lfy:request#request:request:9de46d56be23f3f490556fce4a860d42c4757c96422d93e16aae07ec44043eff
#[test]
fn a_unit_planned_for_its_requirements_marks_what_is_new_and_lists_what_is_gone() {
    let fixture = a_and_b();
    // `b` as it was: `make` carried a criterion about an empty `x` and one about a short
    // one. It carries the first and a third now, so the second is gone and the third new.
    let was = B.replace(
        "@test(",
        "@acceptanceCriteria.add({ situation = `x is short`, behavior = `the b is empty` });\n  \
         @test(",
    );
    // The third is written before the first, so no criterion of the unit stands where a
    // criterion the outputs answered for stood: nothing is reworded here, and the second
    // is gone rather than reworded.
    let now = B.replace(
        "@acceptanceCriteria.add({ situation = `x is empty`",
        "@acceptanceCriteria.add({ situation = `x is long`, behavior = `the b is cut` });\n  \
         @acceptanceCriteria.add({ situation = `x is empty`",
    );
    fixture.write("def/b.lfy", &was);
    let old = fixture.program();
    assert_bound(&old.workspace);
    let maps = current_maps(&old, &plan_of(&old, &[], &[], &[]));
    let before: Vec<String> = {
        let plan = plan_of(&old, &[], &[], &[]);
        let (_, unit) = unit_of(&old.workspace, &plan, "def/b.lfy");
        unit.requirements(&old)
            .iter()
            .map(|requirement| requirement.id().to_string())
            .collect()
    };

    fixture.write("def/b.lfy", &now);
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &maps, &[], &[]);
    let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
    // The lowered code is the same; only the criteria differ.
    assert_eq!(b.reason, Some(Reason::Requirements));
    let existing = [Output {
        path: "src/b.rs".to_string(),
        text: "// old".to_string(),
    }];
    let batch = Batch {
        units: vec![b_index],
        identifier: "b".to_string(),
    };
    let previous = BTreeMap::from([("def/b.lfy".to_string(), was.clone())]);
    let request = request(&program, &plan, &batch, &existing, &previous, false, &[]);

    // What the outputs answered for is what the map file recorded.
    // @lfy def/generation/main.lfy:request#request:request:160056c645e3e69afd7ce0bb74137db9a5e5f286df0a983570f9e2569276b533
    let mut sorted = before.clone();
    sorted.sort();
    assert_eq!(request.answered["def/b.lfy"], sorted);
    // The one criterion the unit no longer has, in the order the file as it was gave it,
    // with the id it had, its place in the file as it was, and what it said.
    // @lfy def/generation/main.lfy:request#request:request:19eae5873e1ffc58511457bc5471084a486c137ebe615a8fdf95f3ac0fe43289
    // @lfy def/generation/main.lfy:request#request:request:1b7315cf290fcd29e9e15f54bd98742f52b2943246c4cb0414ed750188b9e3da
    let gone = &request.removed["def/b.lfy"];
    assert_eq!(gone.len(), 1, "{gone:?}");
    assert!(before.contains(&gone[0].id), "{gone:?} {before:?}");
    assert_eq!(gone[0].text, "When x is short: the b is empty");
    // The place is the file as it was and the line its `add` call began on, which is
    // the line the criterion was written on in that file and no line of the file now.
    let line = was
        .lines()
        .position(|line| line.contains("x is short"))
        .expect("the criterion is written in the file as it was")
        + 1;
    assert_eq!(gone[0].place, format!("def/b.lfy:{line}"));

    let text = &request.instructions;
    // The section says the code is unchanged and holds the code to what each criterion
    // now says.
    // @lfy def/generation/main.lfy:request#request:request:dab229dc001954e941ede8f9cf711d6ff5830dd88e08a35f3bccea3da26ad09b
    assert!(
        text.contains("The lowered code of this unit is unchanged, but its criteria or tests \ndiffer.")
            || text.contains(
                "The lowered code of this unit is unchanged, but its criteria or tests differ."
            ),
        "{text}"
    );
    assert!(
        text.contains("must do what each criterion and test now says"),
        "{text}"
    );
    // The criterion the outputs answered for is unmarked; the one they did not is new.
    // @lfy def/generation/main.lfy:request#request:request:d2d5d86c8b26bed5bb851853987bcc7fdd4050b23289d7c04705420520afdb99
    let held = b.requirements(&program);
    let kept = held
        .iter()
        .find(|requirement| before.contains(&requirement.id().to_string()))
        .expect("one criterion or test the outputs answered for");
    let fresh = held
        .iter()
        .find(|requirement| !before.contains(&requirement.id().to_string()))
        .expect("one the outputs did not answer for");
    assert!(
        text.contains(&format!("`{}` — When x is long: the b is cut (new)", fresh.id())),
        "{text}"
    );
    assert!(!text.contains(&format!("`{}` — ", kept.id())) || {
        let line = text
            .lines()
            .find(|line| line.contains(kept.id()))
            .unwrap_or("");
        !line.ends_with("(new)")
    });
    // A new one may restate a removed one in other words.
    // @lfy def/generation/main.lfy:request#request:request:40e519bc14a840e00c6e0fb7bff68febae9b5b591f182632fddc0a29f6f47704
    assert!(
        text.contains("may restate a removed one in other words"),
        "{text}"
    );
    // What is gone is listed last in the unit's section, with its id and its text.
    // @lfy def/generation/main.lfy:request#request:request:11d52a6cfc7c6727d63a4b8f9330ce26ffeb26adddad76ca0ec87d826a89e8b2
    let heading = text
        .find("#### Removed since its outputs were accepted")
        .unwrap_or_else(|| panic!("{text}"));
    assert!(
        text[heading..].contains(&format!("`{}` — When x is short: the b is empty", gone[0].id)),
        "{text}"
    );
    assert!(heading < text.find("## Global criteria and tests").unwrap());
    // No criterion of the unit stands where one the outputs answered for stood, so
    // nothing was reworded, and the unit asks for more than its markers being moved.
    // @lfy def/generation/main.lfy:request#request:request:de823e5d8fbff081ee17b88c1ae0477dff3832d78d636c05c9b9a8c90ae71da7
    // @lfy def/generation/main.lfy:request#request:request:d1ec465e091ae83573ab2ee5e7fe9ee86667ea44ffe505f8aefe88311e0071b7
    assert!(
        request.reworded["def/b.lfy"].is_empty(),
        "{:?}",
        request.reworded
    );
    assert!(
        request.reworded_only.is_empty(),
        "{:?}",
        request.reworded_only
    );
    assert!(!text.contains("(reworded)"), "{text}");
}

/// A unit whose one criterion was reworded in place: it is marked reworded rather than
/// new, the id and the text it replaces follow it, nothing is listed as removed, and the
/// unit is one whose outputs differ only in that id.
// @lfy def/generation/main.lfy:request#request:request:d003b615015419f91865b0d43a16202327e5faa27462bc24af60cc34549af280
#[test]
fn a_unit_whose_criterion_was_reworded_in_place_marks_it_and_says_what_it_replaces() {
    let fixture = a_and_b();
    // `b` as it was: `make` carried the criterion about an empty `x` and a second one
    // about a short one. The second is worded differently now, in the same place, so it
    // keeps its place and takes a new id.
    let criterion = |situation: &str, behavior: &str| {
        B.replace(
            "@test(",
            &format!(
                "@acceptanceCriteria.add({{ situation = `{situation}`, behavior = \
                 `{behavior}` }});\n  @test("
            ),
        )
    };
    let was = criterion("x is short", "the b is empty");
    let now = criterion("x is brief", "the b is bare");
    fixture.write("def/b.lfy", &was);
    let old = fixture.program();
    assert_bound(&old.workspace);
    let maps = current_maps(&old, &plan_of(&old, &[], &[], &[]));
    let replaced: String = {
        let plan = plan_of(&old, &[], &[], &[]);
        let (_, unit) = unit_of(&old.workspace, &plan, "def/b.lfy");
        unit.requirements(&old)
            .iter()
            .map(|requirement| requirement.id().to_string())
            .nth(1)
            .expect("the second criterion of the file as it was")
    };

    fixture.write("def/b.lfy", &now);
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &maps, &[], &[]);
    let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
    assert_eq!(b.reason, Some(Reason::Requirements));
    let existing = [Output {
        path: "src/b.rs".to_string(),
        text: "// old".to_string(),
    }];
    let batch = Batch {
        units: vec![b_index],
        identifier: "b".to_string(),
    };
    let previous = BTreeMap::from([("def/b.lfy".to_string(), was.clone())]);
    let request = request(&program, &plan, &batch, &existing, &previous, false, &[]);

    // One rewording: the id the unit has now, the id it replaces, and what that one
    // said, taken from the lowering of the file as it was.
    // @lfy def/generation/main.lfy:request#request:request:de823e5d8fbff081ee17b88c1ae0477dff3832d78d636c05c9b9a8c90ae71da7
    let rewordings = &request.reworded["def/b.lfy"];
    assert_eq!(rewordings.len(), 1, "{rewordings:?}");
    assert_eq!(rewordings[0].replaces, replaced);
    assert_eq!(rewordings[0].text, "When x is short: the b is empty");
    let reworded = b
        .requirements(&program)
        .iter()
        .map(|requirement| requirement.id().to_string())
        .nth(1)
        .expect("the second criterion of the unit");
    assert_eq!(rewordings[0].id, reworded);
    // What was reworded is not gone, so nothing is listed as removed.
    // @lfy def/generation/main.lfy:request#request:request:19eae5873e1ffc58511457bc5471084a486c137ebe615a8fdf95f3ac0fe43289
    assert!(
        request.removed["def/b.lfy"].is_empty(),
        "{:?}",
        request.removed
    );
    // Nothing but that id differs from what the outputs were generated against.
    // @lfy def/generation/main.lfy:request#request:request:4a0dea1f763a8af062fa2366467ce5030e12d175192b8584660d6f3ace3eebc6
    assert_eq!(request.reworded_only, ["def/b.lfy"]);

    let text = &request.instructions;
    // The line is marked reworded, not new, and the id and text it replaces follow it.
    // @lfy def/generation/main.lfy:request#request:request:e1d45d2b90e4f031fa2c8711633c2ef9ff914b2ca15467eee998d20246c6a3d9
    // @lfy def/generation/main.lfy:request#request:request:d2d5d86c8b26bed5bb851853987bcc7fdd4050b23289d7c04705420520afdb99
    assert!(
        text.contains(&format!(
            "`{reworded}` — When x is brief: the b is bare (reworded)\n  replaces {replaced} \
             — When x is short: the b is empty\n"
        )),
        "{text}"
    );
    assert!(!text.contains(&format!("`{reworded}` — When x is brief: the b is bare (new)")));
    // The section says the markers have been moved already and what is left to read.
    // @lfy def/generation/main.lfy:request#request:request:5dcbf4d56e0e083dd49457b03e35a549d6a72d10894e442c3a9f608dba553186
    for part in [
        "is rewritten to name \nits new id",
        "compile has done so before you run",
        "against what it says now",
    ] {
        assert!(
            text.contains(part) || text.contains(&part.replace(" \n", " ")),
            "{part} is missing:\n{text}"
        );
    }
    // Nothing is marked new, so the section does not speak of a restated removed one.
    // @lfy def/generation/main.lfy:request#request:request:40e519bc14a840e00c6e0fb7bff68febae9b5b591f182632fddc0a29f6f47704
    assert!(!text.contains("may restate a removed one in other words"), "{text}");
    assert!(
        !text.contains("#### Removed since its outputs were accepted"),
        "{text}"
    );
}

/// Where the text handed over for a unit is only its hash-matched source, nothing of that
/// revision says what its outputs answered for: no criterion of the unit rewords one of
/// its, what is gone is read off the recorded ids alone — with neither a place nor a text,
/// which the instructions say — and the unit is not one whose markers are the whole of the
/// work.
// @lfy def/generation/main.lfy:request#request:request:68bd098b5b9b98c6a8f66b7bebfe21fd86dc2ead715c655d6f9f940a6408ff30
// @lfy def/generation/main.lfy:request#request:request:fad31483b1a56330cc1438cb3020ce5c42a81a5a92c5eb382ce0929be2df4eaa
// @lfy def/generation/main.lfy:request#request:request:4a0dea1f763a8af062fa2366467ce5030e12d175192b8584660d6f3ace3eebc6
#[test]
fn a_hash_matched_source_gives_the_recorded_ids_alone() {
    let fixture = a_and_b();
    let criterion = |situation: &str, behavior: &str| {
        B.replace(
            "@test(",
            &format!(
                "@acceptanceCriteria.add({{ situation = `{situation}`, behavior = \
                 `{behavior}` }});\n  @test("
            ),
        )
    };
    let was = criterion("x is short", "the b is empty");
    let now = criterion("x is brief", "the b is bare");
    fixture.write("def/b.lfy", &was);
    let old = fixture.program();
    assert_bound(&old.workspace);
    let maps = current_maps(&old, &plan_of(&old, &[], &[], &[]));
    let replaced: String = {
        let plan = plan_of(&old, &[], &[], &[]);
        let (_, unit) = unit_of(&old.workspace, &plan, "def/b.lfy");
        unit.requirements(&old)
            .iter()
            .map(|requirement| requirement.id().to_string())
            .nth(1)
            .expect("the second criterion of the hash-matched source")
    };

    fixture.write("def/b.lfy", &now);
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &maps, &[], &[]);
    let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
    assert_eq!(b.reason, Some(Reason::Requirements));
    let existing = [Output {
        path: "src/b.rs".to_string(),
        text: "// old".to_string(),
    }];
    let batch = Batch {
        units: vec![b_index],
        identifier: "b".to_string(),
    };
    let previous = BTreeMap::from([("def/b.lfy".to_string(), was.clone())]);
    let matched = ["def/b.lfy".to_string()];
    let request = request(
        &program,
        &plan,
        &batch,
        &existing,
        &previous,
        false,
        &matched,
    );

    assert!(
        request.reworded["def/b.lfy"].is_empty(),
        "{:?}",
        request.reworded
    );
    // The ids the maps recorded that the unit no longer has, in id order, with neither a
    // place nor a text.
    let gone = &request.removed["def/b.lfy"];
    assert_eq!(
        gone.iter().map(|one| one.id.as_str()).collect::<Vec<_>>(),
        [replaced.as_str()]
    );
    assert_eq!(gone[0].place, "");
    assert_eq!(gone[0].text, "");
    assert!(gone.windows(2).all(|pair| pair[0].id <= pair[1].id));
    assert!(
        request.reworded_only.is_empty(),
        "{:?}",
        request.reworded_only
    );

    let text = &request.instructions;
    assert!(
        text.contains(&format!("`{replaced}` — what it said is not known")),
        "{text}"
    );
    assert!(!text.contains("(reworded)"), "{text}");
}

