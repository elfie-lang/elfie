// @lfy def/generation/main.lfy:request
//! Tests of `request`.

use super::*;

/// The part of a request's instructions that begins with a heading, up to the next
/// heading of the same level or the end.
fn part<'t>(text: &'t str, heading: &str) -> &'t str {
    let at = text
        .find(heading)
        .unwrap_or_else(|| panic!("{heading} is missing:\n{text}"));
    let rest = &text[at..];
    let end = rest[1..]
        .find("\n## ")
        .map_or(rest.len(), |found| found + 1);
    &rest[..end]
}

// @lfy def/generation/main.lfy:request#request:request:791a8be2cf25fb1173708afcc97731764294fd1536d2712c435d59d1f67ab210
#[test]
fn a_request_names_the_target_and_the_unit_and_quotes_its_entities() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
    let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
    let request = request(&program, &plan, a_index, &[], None, false, false, &[]);
    // @lfy def/generation/main.lfy:request#request:request:959d0fbacdba0aea498605d429f4ef8f02ed09ff514957b1eb5c5d9cd0244bf9
    assert_eq!(request.unit, a_index);
    // The source is the lowered text of the unit's file, and the requirements its local
    // criteria and tests.
    // @lfy def/generation/main.lfy:request
    assert_eq!(request.source, program.files[a.lowered].text);
    assert!(request.source.contains("d A: `An a` {"), "{}", request.source);
    assert!(request.requirements.is_empty(), "{:?}", request.requirements);
    // Nothing of this project is added to global, so there is no global requirement.
    // @lfy def/generation/main.lfy:request
    assert!(request.globals.is_empty(), "{:?}", request.globals);
    // Nothing was handed over of what the outputs were generated from.
    assert_eq!(request.previous, None);
    assert_eq!(request.answered, None);
    assert_eq!(request.removed, None);
    assert_eq!(request.reworded, None);
    assert!(!request.reworded_only);
    // @lfy def/generation/main.lfy:request
    assert!(request.existing.is_empty());
    assert!(request.foreign_edits.is_empty());
    // a is the last unit, so the run may edit no other unit's code.
    // @lfy def/generation/main.lfy:request#request:request:3fe3499800670559a48a3f228ba09e462416bfcd313e21d86ea7c91319ba2856
    assert!(request.reach.units.is_empty(), "{:?}", request.reach);
    // b is a's dependency, and so its interface.
    // @lfy def/generation/main.lfy:request
    assert_eq!(request.interfaces.len(), 1);
    assert_eq!(request.interfaces[0].unit, b_index);
    // The declaration has no criterion of its own, the layout and the ecosystem none
    // either, and the language one; `target` is all that they extend, and it has none.
    // @lfy def/generation/main.lfy:request
    assert_eq!(
        request.guidance,
        guidance_for(&workspace.model, &workspace.targets[0])
    );
    assert_eq!(request.guidance.len(), 1);
    // What the target's own dependencies, commands, and knowledge give.
    // @lfy def/generation/main.lfy:request#request:request:135e15c4022af725fbc8ead4da685356a7bd57007a779d43dc620fb979b49d4e
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
    // The role: the compiler for the target, whose whole job is the one unit's outputs.
    // @lfy def/generation/main.lfy:request#request:request:fff6f01c8ce622f973502bc046ee6f471011a0368d5ba7c0c0c0a4f4c94a8e0c
    assert!(text.contains("You are the compiler for the target `rust`."), "{text}");
    assert!(
        text.contains("Your whole job is the outputs of the one unit"),
        "{text}"
    );
    assert!(
        text.contains("you write nothing outside them, except the edits to later units"),
        "{text}"
    );
    // @lfy def/generation/main.lfy:request#request:request:ebef3352a48540ef0a12abd094f973ab2762f62746edd431a7b3d4d95f8c3780
    assert!(
        text.contains("The outputs go under `src`, their paths spelled from the unit's stem"),
        "{text}"
    );
    // The unit's section begins with a heading naming the stem, the file, and the reason.
    // @lfy def/generation/main.lfy:request#request:request:61e677b44b6f0c053f64e20b7685d271519f83515df009d807762ca548cbfbeb
    assert!(
        text.contains(
            "## The unit to compile: `a` (`def/a.lfy`), planned because it is fresh"
        ),
        "{text}"
    );
    // @lfy def/generation/main.lfy:request#request:request:f2a22f45bc9513dedff93a88f20ba324a34cd2dbadc5b117686df4eb14f73a47
    assert!(text.contains("Nothing has been generated for it yet."), "{text}");
    assert!(text.contains("### `A` (data: DataDeclaration)"), "{text}");
    // The entity's lowered code, not its source: the documentation is gone.
    // @lfy def/generation/main.lfy:request#request:request:53047b9ea4a7fce9d7173c9b614c2c73247779aa5d5245a9720c351789d608c1
    assert!(text.contains("d A: `An a` {\n  $b = B;\n}"), "{text}");
    assert!(text.contains("Definition: An a"), "{text}");
    assert!(
        text.contains("Each unit becomes one module named after its stem"),
        "{text}"
    );
    assert!(text.contains("`serde_json` (cargo), version `1`"), "{text}");
    assert!(text.contains("`sha2` (cargo), any version"), "{text}");
    // The interface of b is quoted whole.
    assert!(text.contains("### `def/b.lfy`"), "{text}");
    // Markers name entities, never lines, and sit in a line comment beginning with the
    // target's marker comment.
    // @lfy def/generation/main.lfy:request#request:request:671617e25d98e36f8c646b0c76d941034839245f2544e239cb4ede3581bb2f33
    assert!(text.contains("Never write a line number."), "{text}");
    assert!(
        text.contains("a line comment beginning with `//`, then `@lfy`"),
        "{text}"
    );
    // The three report lines.
    // @lfy def/generation/main.lfy:request#request:request:e4ff528873311ac2c054d3eb98c7015eca5064897eae15ff47429cfdd53b7860
    assert!(text.contains("`ELFIE: DONE`"), "{text}");
    assert!(text.contains("`ELFIE: BLOCKED: `"), "{text}");
    assert!(text.contains("`ELFIE: CLARIFY: `"), "{text}");
    // The standard library is bound, never generated.
    // @lfy def/generation/main.lfy:request
    assert!(
        text.contains("carry `builtin` are bound to what the guidance"),
        "{text}"
    );
    // A criterion is never restated in an output, only named by its id.
    // @lfy def/generation/main.lfy:request#request:request:88bad81c76d679b95e82ea566e65627b4a1615584da2fe8b645fd322f0b26fb3
    assert!(
        text.contains("Never restate a criterion or a test in an output"),
        "{text}"
    );
    // The commands run with `sh -c` in the output directory, and a dependency is added
    // only by running the add command.
    // @lfy def/generation/main.lfy:request#request:request:09b0190ee8969e01b9ff7a561a98c3663f87fb8b97fa83c588d0631e7989bc20
    assert!(text.contains("These run with `sh -c` in `src`"), "{text}");
    assert!(
        text.contains("never by editing a manifest or a lockfile by hand"),
        "{text}"
    );
    assert!(
        text.contains("The build, test, and lint commands are run before the report"),
        "{text}"
    );
    // No foreign edit and no existing output, so neither part is there.
    // @lfy def/generation/main.lfy:request#request:request:5c766480f2aeaf0ebb1ddf1b6a70d47e9bf4cd4b053467207c3a9d2d1eb8b527
    assert!(!text.contains("## Foreign edits"), "{text}");
    // @lfy def/generation/main.lfy:request#request:request:f2c797c5d0ef378166c8076e5ab2728b7f37655f9e7217e2c879e60c4694babf
    assert!(!text.contains("## Existing outputs"), "{text}");
    // @lfy def/generation/main.lfy:request#request:request:1fe812f0a1e93e2969e57f613ae31f818a433b471cecffbd6f97146481273ba3
    assert!(
        text.contains("This run edits the outputs of no other unit."),
        "{text}"
    );
    // The fixture's language sets no stub line, so a later unit is edited only by a fix.
    // @lfy def/generation/main.lfy:request#request:request:fa7877d32be5d508227d6d6bf22128a10d3041153bcb8b6aee2e60a4efad0c5b
    assert!(
        text.contains("The target has no stub line, so a later unit's code is edited only by a fix."),
        "{text}"
    );

    // The parts come in the definition's order.
    // @lfy def/generation/main.lfy:request#request:request:0efe0565880b5d7bbc83ee22c7e44e71d1599754ad1d82c9ea4fc7fd4bda4282
    let headings = [
        "# Compiling",
        "## Where the outputs go",
        "## Guidance",
        "## Native dependencies",
        "## Knowledge of the target",
        "## Commands",
        "## Global criteria and tests",
        "## Criteria and tests in an output",
        "## The standard library",
        "## Rules for kinds",
        "## Markers",
        "## Tests",
        "## Stubs and fixes in later units",
        "## The agent server",
        "## How to report",
        "## Interfaces",
        "## Knowledge of the unit",
        "## The unit to compile",
        "## The reach of this run",
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

/// Every part before the interfaces reads the same in the request for every unit of the
/// target made from one plan with the same `hosted`, so it is cached across units.
// @lfy def/generation/main.lfy:request#request:request:eb00a710ef7f9ae5f807e0eee8c047af03c75306a424a1d098caac52a1699eb1
#[test]
fn every_part_before_the_interfaces_is_the_same_for_every_unit() {
    let fixture = a_and_b();
    let program = fixture.program();
    assert_bound(&program.workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    for hosted in [false, true] {
        let heads: Vec<String> = (0..plan.units.len())
            .map(|index| {
                let text = request(&program, &plan, index, &[], None, hosted, false, &[])
                    .instructions;
                let at = text.find("## Interfaces").expect("an interfaces part");
                text[..at].to_string()
            })
            .collect();
        assert_eq!(heads.len(), 2);
        assert_eq!(heads[0], heads[1]);
        assert!(!heads[0].contains("def/a.lfy"), "{}", heads[0]);
        assert!(!heads[0].contains("def/b.lfy"), "{}", heads[0]);
    }
}

// @lfy def/generation/main.lfy:request#request:request:258d7f943a5b84ea809c220868bbf11249a423cd3891bd01fced30a1d9b5bfc1
#[test]
fn a_request_quotes_every_criterion_and_test_of_an_entity() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (b_index, unit) = unit_of(&workspace, &plan, "def/b.lfy");
    let request = request(&program, &plan, b_index, &[], None, false, false, &[]);
    // The requirements hold every local criterion and test of the file, each with its
    // id: one criterion and one test, both of `make`.
    // @lfy def/generation/main.lfy:request
    assert_eq!(request.requirements.len(), 2);
    assert!(
        request
            .requirements
            .iter()
            .all(|requirement| requirement.id().starts_with("make:make:")),
        "{:?}",
        request.requirements
    );
    // The body of a fn is its criteria and tests, which are read rather than generated,
    // so what is left of one in the source is its signature.
    assert!(
        request
            .source
            .contains("fn make(x: string): `Makes a b` => B;"),
        "{}",
        request.source
    );
    let text = &request.instructions;
    assert!(
        text.contains("### `make` (agent function: AgentFunctionDeclaration)"),
        "{text}"
    );
    // Each criterion and each test is on its own line: its id, an em dash, and its text.
    let entity = *unit.entities.last().expect("make is an entity of b");
    let (criteria, tests) = requirement_ids(&program.files[unit.lowered], entity);
    assert!(
        text.contains(&format!("- {} — When x is empty: the b holds x\n", criteria[0])),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "- {} — Input `[\"y\"]` gives `B@like(`holding y`)`\n",
            tests[0]
        )),
        "{text}"
    );
}

/// Every global criterion and test of the program is in one section of its own, each
/// once with its id, and the instructions say what a unit does with one.
// @lfy def/generation/main.lfy:request#request:request:947030aa711f453c710b970e654a978a41f4db5a2776707bc669e1c0efd5bea8
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
    let request = request(&program, &plan, 0, &[], None, false, false, &[]);
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
            "- {} — Nothing is written twice",
            program.criteria[0].id
        )),
        "{text}"
    );
    assert!(text.contains("These hold across the whole program"), "{text}");
    assert!(
        text.contains("the global review checks each one once"),
        "{text}"
    );
    // It is not in the unit's own section: a local criterion and a global one are kept
    // apart.
    assert_eq!(text.matches(program.criteria[0].id.as_str()).count(), 1, "{text}");
}

/// A target whose declaration is given a criterion by an add call at the top of its
/// file, whose layout layer and whose language layer each hold criteria, whose layout
/// layer extends `targetFramework` too and fills the frameworks slot as well, and whose
/// language is applied with an argument: the guidance is what no layer and no trait a
/// layer extends gave, then each layer's in guidance order, then those of every trait
/// the layers extend, nearest first and each trait once, with every template value
/// rendered from the layer's arguments.
// @lfy def/generation/main.lfy:request#request:request:87e9993504e790d854b0888818bd3cd0f6e6cabc4fe778e8ca201843373bbc6a
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
    assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
    let request = request(&program, &plan, 0, &[], None, false, false, &[]);
    let behaviors: Vec<String> = request
        .guidance
        .iter()
        .map(|criterion| criterion.behavior.clone().unwrap_or_default().join(" "))
        .collect();
    // What the file's add call gave first, since no layer and no trait a layer extends
    // gave it; then the layout and the language, in guidance order, with the language's
    // template value rendered from its argument; then the traits they extend, nearest
    // first.
    // @lfy def/generation/main.lfy:request#request:request:135e15c4022af725fbc8ead4da685356a7bd57007a779d43dc620fb979b49d4e
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
    // Every criterion is quoted in the instructions, in that order, and no template is
    // left in the guidance.
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
    assert!(!text.contains("{{edition}}"), "{text}");
}

/// The knowledge of the unit's entities and of the entities of an interface, each item
/// once, in the unit's knowledge part: a reference quoted in full, one named to be read, a
/// definition as the files its use path resolves to, and a tool of the agent server.
// @lfy def/generation/main.lfy:request#request:request:c98e56391cddb2a9c1906590ced0e660422b844fd0eaf115ecabe5c0d384a131
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
    let request = request(&program, &plan, a_index, &[], None, false, false, &[]);
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
    // The target gives none of its own; every item is the unit's.
    // @lfy def/generation/main.lfy:request#request:request:52ca7fc29e1e450ee23c263741df61d61b239c73a255a810b7d879c5cf38f7ac
    assert!(
        part(text, "## Knowledge of the target")
            .contains("The compiler is given nothing of this kind to read."),
        "{text}"
    );
    let unit_part = part(text, "## Knowledge of the unit");
    // A reference whose quote is true is quoted in full, from its source joined to the
    // root of the project its contributor is declared in.
    // @lfy def/generation/main.lfy:request#request:request:f64fa963c69a9e30aa2c190c595acdbb6221274d050a5a8533b9cf9291684155
    assert!(unit_part.contains("From `docs/style.md`:"), "{text}");
    assert!(unit_part.contains("Two spaces, never a tab."), "{text}");
    // One whose quote is left out is named, to be read first.
    // @lfy def/generation/main.lfy:request#request:request:737e4198f2f6bddc790512ae099b13648ed59405ba19506fb0abff20748ec530
    assert!(
        unit_part.contains(
            "- the shape of a module: read `docs/shape.md` before writing anything it covers."
        ),
        "{text}"
    );
    assert!(!text.contains("A long guide nobody quotes."), "{text}");
    // A definition names the files its use path resolves to, read as Elfie.
    // @lfy def/generation/main.lfy:request#request:request:179394ea2be57b4a87b41dac2988b30f36b058129a701fc79ff34cc290769606
    assert!(
        unit_part.contains("- what a b is: read `def/b.lfy` as Elfie."),
        "{text}"
    );
    // A tool is called before anything relies on what it answers.
    // @lfy def/generation/main.lfy:request#request:request:18fa49f6ae0fe059f4d8b65429f7356e822363b0bb901d611f266ca1393c940b
    assert!(
        unit_part.contains(
            "- the program: call the tool `elfie_entity` of the agent server before relying on \
             what it answers."
        ),
        "{text}"
    );
}

// @lfy def/generation/main.lfy:request#request:request:1530126fe56400a7920d4c5c53eb10a8785a3ed3921d00bc2814a0452615fd57
// @lfy def/generation/main.lfy:request#request:request:7424b0fcb62916f23bfb6c1265f3a3f943fb1354558e0d4b0ea4722212bc0c7f
#[test]
fn a_dependency_is_an_interface_with_its_outputs_and_entities() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let maps = current_maps(&program, &plan_of(&program, &[], &[], &[]));
    let plan = plan_of(&program, &maps, &["a".to_string()], &[]);
    let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
    let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
    let request = request(&program, &plan, a_index, &[], None, false, false, &[]);
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
/// `elfie_task`, `elfie_mark`, and `elfie_reach`, its stubs part `elfie_touch`, and its
/// report part `elfie_submit` and the three lines.
// @lfy def/generation/main.lfy:request#request:request:88959b8d48ca3a336e3c22cd37a1dc460e6e8d3ea8f5f2f7946e3defb0b28501
#[test]
fn a_hosted_request_names_its_interfaces_and_the_tools_of_its_session() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let maps = current_maps(&program, &plan_of(&program, &[], &[], &[]));
    let plan = plan_of(&program, &maps, &["a".to_string()], &[]);
    let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
    let request = request(&program, &plan, a_index, &[], None, true, false, &[]);
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
    // No definition, kind, or type of an entity of the interface is spelled out.
    assert!(!text.contains("### `def/b.lfy`"), "{text}");
    assert!(!text.contains("(data: DataDeclaration): A b"), "{text}");
    // @lfy def/generation/main.lfy:request#request:request:91c07c652cfc9edd8b82e5c1bebc5ef3998ae8e528cbd1623161418170399772
    assert!(text.contains("with `elfie_dependency`"), "{text}");
    // @lfy def/generation/main.lfy:request#request:request:ee02a52de55ba8387ed07c92e9dcbdf5b9c85589d8fdaada91f00910e1406e76
    for piece in [
        "`elfie_task` gives the unit's obligations again: its outputs and its reason",
        "marked new, kept, or unknown",
        "`elfie_mark` writes a marker line",
        "rewrites the id every marker of the unit's outputs names",
        "never type an id by hand",
        "`elfie_reach` gives the units the run may edit, their regions, and what is left of \
         the fix cap",
    ] {
        assert!(text.contains(piece), "{piece} is missing:\n{text}");
    }
    // @lfy def/generation/main.lfy:request#request:request:62d883a30a28badaea4f03b2960f52fb3aa9c70f5050d0dbf476d7a7f1acbd15
    assert!(
        part(text, "## Stubs and fixes in later units").contains("`elfie_touch`"),
        "{text}"
    );
    // @lfy def/generation/main.lfy:request#request:request:4a237f69a0cf92b366172dde838e3583135a0dc294c8ef43f2392d08c1c1e1bd
    assert!(
        text.contains("Submit the outcome through `elfie_submit` before your final message"),
        "{text}"
    );
    for line in ["`ELFIE: DONE`", "`ELFIE: BLOCKED: `", "`ELFIE: CLARIFY: `"] {
        assert!(text.contains(line), "{line} is missing:\n{text}");
    }

    // A request that is not hosted quotes the interface whole and names none of the
    // session's own tools.
    // @lfy def/generation/main.lfy:request#request:request:1530126fe56400a7920d4c5c53eb10a8785a3ed3921d00bc2814a0452615fd57
    let text =
        super::request(&program, &plan, a_index, &[], None, false, false, &[]).instructions;
    assert!(text.contains("### `def/b.lfy`"), "{text}");
    assert!(text.contains("- `B` (data: DataDeclaration): A b"), "{text}");
    for tool in ["elfie_dependency", "elfie_task", "elfie_mark", "elfie_reach", "elfie_submit"] {
        assert!(!text.contains(tool), "{tool}:\n{text}");
    }
}

/// The commands part says which operations the agent server runs through a tool of its
/// own, which variables narrow such a run, and how the unit is judged after its run.
// @lfy def/generation/main.lfy:request#request:request:f700bbb14b11b81fadfe6c18b018250b1c7b9cdadbd0bd8b2a66ce896dfc54c9
// @lfy def/generation/main.lfy:request#request:request:f4b1430a5e2784e149775d2e69bdf4d652782d426521aa999f67d64b6c8a76c5
#[test]
fn the_commands_part_says_which_tool_runs_an_operation_and_what_narrows_it() {
    let fixture = a_and_b();
    let program = fixture.program();
    assert_bound(&program.workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let text = request(&program, &plan, 0, &[], None, false, false, &[]).instructions;
    let commands = part(&text, "## Commands");
    for piece in [
        "Where the agent server offers `elfie_build`, `elfie_test`, `elfie_lint`, or \
         `elfie_format`, the command of that operation is run through that tool.",
        "Run a line by hand only for an operation whose tool is not offered.",
        "The filter of `elfie_test` is given to the test command as `TESTS`",
        "the scope of `elfie_build`, `elfie_test`, and `elfie_lint` is given to the command \
         as `SCOPE`",
        "narrows the run only where the command's line reads it",
    ] {
        assert!(commands.contains(piece), "{piece} is missing:\n{text}");
    }
    // @lfy def/generation/main.lfy:request#request:request:e198f2bcd4f8eb39c9c181816e09b176b6436e859f6c7e7015ba21dcf9e0f4ca
    for piece in [
        "the unit is judged with the test command run with stubs allowed",
        "`ALLOW_STUBS`",
        "`allowStubs` of `elfie_test`",
        "a test failing only by reaching a stub in a later unit's code does not fail the unit",
        "A lint warning fails the unit only in the unit's own code.",
    ] {
        assert!(commands.contains(piece), "{piece} is missing:\n{text}");
    }
    // The tools part names every tool of the agent server a compiler reads the program
    // and its outputs through.
    // @lfy def/generation/main.lfy:request#request:request:43974a3eb5968199e4c0d2b5b84afd218c864eb58c47c072bbcc471866a03921
    for piece in [
        "before files are searched or read by hand",
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
        assert!(text.contains(piece), "{piece} is missing:\n{text}");
    }
}

/// The section of a unit entities moved into says which they are, from which file, and
/// because of which cause, and the section of the unit they left says which they are and
/// which unit compiles them.
// @lfy def/generation/main.lfy:request#request:request:da09691bed9bbd4cea507b74ae9d6919801f1f5948f2c6d2728226a9c8d5f73f
// @lfy def/generation/main.lfy:request#request:request:0f1d87f11591131ce4052dfc72bc179de20f377a5b220fb4cb1cd3f9237e0d4e
#[test]
fn the_section_of_a_unit_says_what_moved_into_it_and_out_of_it() {
    let fixture = a_cycle();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
    let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");

    // In the request of `def/b.lfy`, which `C` moved into.
    let into = request(&program, &plan, b_index, &[], None, false, false, &[]);
    let text = &into.instructions;
    assert!(
        text.contains("- `C` from `def/a.lfy`, because of `B.c`."),
        "{text}"
    );
    // @lfy def/generation/main.lfy:request#request:request:e0d2c5750e61bf64facae6b21f456aaeafa4cfd1be0dc60ff387efee1c092a88
    assert!(
        text.contains("markers naming the file that declares them"),
        "{text}"
    );
    // @lfy def/generation/main.lfy:request#request:request:ac5358d47d574ae4484013ee8bdc0488e7f6ce892b6e384d1346f27daea31d51
    assert!(text.contains("never by importing that file"), "{text}");
    // The source of the unit is the text it compiles, so `C` is in it.
    // @lfy def/generation/main.lfy:request
    assert!(into.source.contains("d C"), "{}", into.source);

    // In the request of `def/a.lfy`, which `C` moved out of.
    let out = request(&program, &plan, a_index, &[], None, false, false, &[]);
    let text = &out.instructions;
    assert!(
        text.contains("- `C` in the unit `b` (`def/b.lfy`)."),
        "{text}"
    );
    // @lfy def/generation/main.lfy:request#request:request:b30f51561b31df6c26b31d4c397791d154398bc31a9a56efc45eb145fae5800a
    assert!(text.contains("never write them again"), "{text}");
    // @lfy def/generation/main.lfy:request#request:request:034c1bdfc4725cdfe7e5eb86af7e47ed43e227a1f7eef528cbb549521620143e
    assert!(
        text.contains("under the name and path it would have had"),
        "{text}"
    );
    assert!(!out.source.contains("d C"), "{}", out.source);
}

/// An existing output is kept only where its path is among the unit's outputs, and the
/// previous source it was generated from is the file as it was, lowered the way the unit's
/// own source is, so that the two texts can be read side by side.
// @lfy def/generation/main.lfy:request#request:request:403680d3f5bf2a057752d5d237cbfb6d63463cc8feff3f3e51a12a290e249b3e
#[test]
fn existing_outputs_are_kept_only_where_they_are_among_the_units_outputs() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let maps = current_maps(&program, &plan_of(&program, &[], &[], &[]));
    let plan = plan_of(&program, &maps, &["a".to_string()], &[]);
    let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
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
    let previous = "use \"./b\";\n\nd A: `An a` {\n  $b = B;\n}\n";
    let with_previous = request(
        &program,
        &plan,
        a_index,
        &existing,
        Some(previous),
        false,
        false,
        &[],
    );
    assert_eq!(with_previous.existing.len(), 1);
    assert_eq!(with_previous.existing["src/a.rs"], "// old");
    // `Request.previous` is the lowered text of the file as it was, never the text as
    // written: it is read against `Request.source`, which is lowered too.
    // @lfy def/generation/main.lfy:request#request:request:f716d0d680ccdf61f0d7552cc5067d5a325199077fe3d5d7280c889ed2df6c4b
    let was = with_previous.previous.clone().expect("a previous source");
    assert_ne!(was, with_previous.source);
    // It is what lowering the file as it was gives: here the same file but for the
    // documentation comment the current one carries.
    // @lfy def/generation/main.lfy:request#request:request:816fbafee677832fddf90f3da69f4b66d0488aa66ef4192fb50ebce3e1fd527d
    let lowered = crate::interpret::lower(crate::workspace::change(
        &workspace,
        "def/a.lfy",
        Some(previous),
    ));
    let index = lowered
        .workspace
        .files
        .iter()
        .position(|file| file.path == "def/a.lfy")
        .unwrap();
    assert_eq!(was, lowered.files[index].text);
    assert!(!was.contains("A record."), "{was}");
    assert!(with_previous.source.contains("A record."));
    let text = &with_previous.instructions;
    let existing_part = part(text, "## Existing outputs");
    // @lfy def/generation/main.lfy:request#request:request:0ba41c4d98f01ad24856a70c3eb94ada370e2dbc756ad6bb03d09c1593aa935b
    assert!(existing_part.contains("- `src/a.rs`"), "{text}");
    assert!(!existing_part.contains("src/other.rs"), "{text}");
    // Only what the difference, the marked criteria and tests, the foreign edits, and a
    // split the guidance requires call for changes; names, structure, and the markers of
    // unchanged items are otherwise kept.
    // @lfy def/generation/main.lfy:request#request:request:49dec1e77860c6eec5de0ebfc0908544ab6d6ecfae3b4877fa2277bf38fd33c2
    for piece in [
        "Keep the markers of unchanged items.",
        "Keep names and structure, except that a module the \
         target's guidance says to split gains submodule files and has its code moved into them: \
         that split is the one change of structure allowed, and it keeps every name other units \
         use.",
        "the criteria and tests marked new, reworded, or removed, the foreign edits, and a split \
         the target's guidance requires, require.",
    ] {
        assert!(existing_part.contains(piece), "{piece} is missing:\n{text}");
    }
    // @lfy def/generation/main.lfy:request#request:request:921f0ecc9962f3241a69e55b5e8db934f9a538bdb6ba94b75df67399bf4e7dcf
    assert!(
        existing_part.contains("The source the outputs were generated from"),
        "{text}"
    );
    assert!(existing_part.contains(was.trim_end()), "{text}");
    // The outputs of a unit that has some go at the paths its `Unit.outputs` names, except
    // that a module the guidance says to split gains submodule files.
    // @lfy def/generation/main.lfy:request#request:request:09f94a5ded65108d10253c1fb3262ce6174fd78e63b564b55a231c913cd36a02
    assert!(
        text.contains(
            "Its outputs go at `src/a.rs`, except that a module the target's guidance says to \
             split gains submodule files and has its code moved into them."
        ),
        "{text}"
    );

    // With no previous source, the existing outputs are reconciled with the unit's source.
    // @lfy def/generation/main.lfy:request#request:request:f56fdcbd147d90b70d21fc0a3ca55daee274a966874753ec15588bbcb77df8c2
    let without = request(&program, &plan, a_index, &existing, None, false, false, &[]);
    assert!(
        without
            .instructions
            .contains("Reconcile the existing outputs with the unit's source."),
        "{}",
        without.instructions
    );
}

/// A unit whose outputs recorded no answered ids leaves the request nothing to say which
/// of its criteria and tests are new, so the outputs part says to read the existing code
/// against each of them; with the file as it was handed over, its ids stand in.
// @lfy def/generation/main.lfy:request#request:request:9dc09496cd7e7f35f205f84cbca429a26f53841256c22511554622cf64d3408e
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
    let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
    let existing = [Output {
        path: "src/b.rs".to_string(),
        text: "// old".to_string(),
    }];
    let unknown = request(&program, &plan, b_index, &existing, None, false, false, &[]);
    assert_eq!(unknown.answered, None);
    assert!(
        unknown
            .instructions
            .contains("Which criteria and tests of the unit are new is not known."),
        "{}",
        unknown.instructions
    );
    // The file as it was says which ids the outputs were generated against.
    // @lfy def/generation/main.lfy:request#request:request:c0f16d7718690de14a23abb41bfc4969ec02c0d005d99bc9c10b079f30ed8856
    let known = request(&program, &plan, b_index, &existing, Some(B), false, false, &[]);
    let ids: Vec<String> = b
        .requirements(&program)
        .iter()
        .map(|requirement| requirement.id().to_string())
        .collect();
    assert_eq!(known.answered, Some(ids));
    assert!(
        !known.instructions.contains("are new is not known"),
        "{}",
        known.instructions
    );
}

/// A unit whose lowered code is unchanged but whose criteria differ: its section says so,
/// each criterion the outputs did not answer for is marked new, and each one the unit no
/// longer has is listed under a heading of its own with its text.
// @lfy def/generation/main.lfy:request#request:request:510db0b077303aad0e00dc82c6b846d584f16e9e4460180873eba965900e021d
#[test]
fn a_unit_planned_for_its_requirements_marks_what_is_new_and_lists_what_is_gone() {
    let fixture = a_and_b();
    // `b` as it was: `make` carried a criterion about an empty `x` and one about a short
    // one. It carries a third and the first now, so the second is gone and the third new.
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
    let request = request(&program, &plan, b_index, &existing, Some(&was), false, false, &[]);

    // What the outputs answered for is what the map file recorded.
    // @lfy def/generation/main.lfy:request#request:request:54d1c80b3d1e38ff7f2e58e0cce13959161756838579cc4936f7e34c3efcffab
    let mut sorted = before.clone();
    sorted.sort();
    assert_eq!(request.answered, Some(sorted));
    // The one criterion the unit no longer has, with the id it had, its place in the file
    // as it was, and what it said.
    // @lfy def/generation/main.lfy:request#request:request:58e21a4c2c020a958a277eeb8c36e5eefab75e4338a11c31f253ec773eddf2b4
    // @lfy def/generation/main.lfy:request#request:request:1b7315cf290fcd29e9e15f54bd98742f52b2943246c4cb0414ed750188b9e3da
    let gone = request.removed.clone().expect("what was removed");
    assert_eq!(gone.len(), 1, "{gone:?}");
    assert!(before.contains(&gone[0].id), "{gone:?} {before:?}");
    assert_eq!(gone[0].text, "When x is short: the b is empty");
    let line = was
        .lines()
        .position(|line| line.contains("x is short"))
        .expect("the criterion is written in the file as it was")
        + 1;
    assert_eq!(gone[0].place, format!("def/b.lfy:{line}"));

    let text = &request.instructions;
    // The section says the code is unchanged and holds the code to what each criterion
    // now says.
    // @lfy def/generation/main.lfy:request#request:request:e9d469e6b5e3e7503987163405e59232553f60e223ea220e7bd04734d2dcfd4c
    assert!(
        text.contains("The lowered code of this unit is unchanged, but its criteria or tests differ."),
        "{text}"
    );
    assert!(
        text.contains("must do what each criterion and test now says"),
        "{text}"
    );
    // The criterion the outputs answered for is unmarked; the one they did not is new.
    // @lfy def/generation/main.lfy:request#request:request:aa208260d2e6de0481fdd00165686f8ac50d4c5662ac4e1445cbaecdfb556b09
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
        text.contains(&format!("- {} — When x is long: the b is cut (new)\n", fresh.id())),
        "{text}"
    );
    let kept_line = text
        .lines()
        .find(|line| line.starts_with(&format!("- {} — ", kept.id())))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(!kept_line.ends_with("(new)"), "{kept_line}");
    // A new one may restate a removed one in other words.
    // @lfy def/generation/main.lfy:request#request:request:1804b6b498c5043da3a7cf8c5a175a457bf933f4f3f4586491df94745de5e1d9
    assert!(
        text.contains("may restate a removed one in other words"),
        "{text}"
    );
    // What is gone is listed last in the unit's section, with its id and its text.
    // @lfy def/generation/main.lfy:request#request:request:345bea3ab85ad7f91097e0f2d2bb6cb5c995e660d46c32999ad0e500adabfaf6
    let unit_part = part(text, "## The unit to compile");
    let heading = unit_part
        .find("### Removed since its outputs were accepted")
        .unwrap_or_else(|| panic!("{text}"));
    assert!(
        unit_part[heading..].contains(&format!("- {} — When x is short: the b is empty", gone[0].id)),
        "{text}"
    );
    // No criterion of the unit stands where one the outputs answered for stood, so
    // nothing was reworded, and the unit asks for more than its markers being moved.
    // @lfy def/generation/main.lfy:request#request:request:7616f4f7b1e0930f9fb2070e7443e11f39ff23318405931148a98d70d772f4a1
    // @lfy def/generation/main.lfy:request#request:request:dcfedcbc95ad64785427d7952c0d6e5a4a7223f1d42d34620e4d3fb6a1330099
    assert_eq!(request.reworded, Some(Vec::new()));
    assert!(!request.reworded_only);
    assert!(!text.contains("(reworded)"), "{text}");
}

/// A unit whose one criterion was reworded in place: it is marked reworded rather than
/// new, the id and the text it replaces follow it, nothing is listed as removed, and the
/// unit is one whose outputs differ only in that id.
// @lfy def/generation/main.lfy:request#request:request:0e580710590d1376564662075d856da231b3b3fca48bccaab37b3985cbfd85fe
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
    let request = request(&program, &plan, b_index, &existing, Some(&was), false, false, &[]);

    // One rewording: the id the unit has now, the id it replaces, and what that one
    // said, taken from the lowering of the file as it was.
    // @lfy def/generation/main.lfy:request#request:request:7616f4f7b1e0930f9fb2070e7443e11f39ff23318405931148a98d70d772f4a1
    let rewordings = request.reworded.clone().expect("what was reworded");
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
    // @lfy def/generation/main.lfy:request#request:request:58e21a4c2c020a958a277eeb8c36e5eefab75e4338a11c31f253ec773eddf2b4
    assert_eq!(request.removed, Some(Vec::new()));
    // Nothing but that id differs from what the outputs were generated against.
    // @lfy def/generation/main.lfy:request#request:request:8ce7bd9048dfd7f8f51bed7605af14f226e8eddbbd5ead98bdabf266257f0593
    assert!(request.reworded_only);

    let text = &request.instructions;
    // The line is marked reworded, not new, and the id and text it replaces follow it.
    // @lfy def/generation/main.lfy:request#request:request:0bc1580cb79537e04f3b1d9cc19ee8db2fb9277b47ffe12994bb0d31371cbee8
    assert!(
        text.contains(&format!(
            "- {reworded} — When x is brief: the b is bare (reworded)\n  replaces {replaced} \
             — When x is short: the b is empty\n"
        )),
        "{text}"
    );
    assert!(!text.contains(&format!("- {reworded} — When x is brief: the b is bare (new)")));
    // The section says the markers have been moved already and what is left to read.
    // @lfy def/generation/main.lfy:request#request:request:3acaed2ffa70c62f8ca37a64447e578819b1c7a7e2d0b480cd8f741b827c3a7e
    for piece in [
        "is rewritten to name its new id",
        "compile has done so before you run",
        "against what it says now",
    ] {
        assert!(text.contains(piece), "{piece} is missing:\n{text}");
    }
    // Nothing is marked new, so the section does not speak of a restated removed one.
    assert!(!text.contains("may restate a removed one in other words"), "{text}");
    assert!(
        !text.contains("### Removed since its outputs were accepted"),
        "{text}"
    );
}

/// Where the text handed over for a unit is only its hash-matched source, nothing of that
/// revision says what its outputs answered for: no criterion of the unit rewords one of
/// its, what is gone is read off the recorded ids alone — with neither a place nor a text,
/// which the instructions say — and the unit is not one whose markers are the whole of the
/// work.
// @lfy def/generation/main.lfy:request#request:request:0b32b60961a7f41d79de0c88090a4493d3a19edcbf8503cf51b580c2215e0ab7
// @lfy def/generation/main.lfy:request#request:request:fad31483b1a56330cc1438cb3020ce5c42a81a5a92c5eb382ce0929be2df4eaa
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
    let request = request(&program, &plan, b_index, &existing, Some(&was), false, true, &[]);

    assert_eq!(request.reworded, Some(Vec::new()));
    // The ids the maps recorded that the unit no longer has, in id order, with neither a
    // place nor a text.
    let gone = request.removed.clone().expect("what was removed");
    assert_eq!(
        gone.iter().map(|one| one.id.as_str()).collect::<Vec<_>>(),
        [replaced.as_str()]
    );
    assert_eq!(gone[0].place, "");
    assert_eq!(gone[0].text, "");
    assert!(!request.reworded_only);

    let text = &request.instructions;
    assert!(
        text.contains(&format!("- {replaced} — what it said is not known")),
        "{text}"
    );
    assert!(!text.contains("(reworded)"), "{text}");
}

/// Three units of one target, `c`, then `b`, then `a`, each with outputs: the run for `b`
/// may edit `a` alone, within the fix cap, and the stubs part gives the stub line the
/// target's language sets as written.
// @lfy def/generation/main.lfy:request#request:request:5a101016477ddbe40d668801c89f3e2c767b1452a4cef6511655c2b5f91666ba
#[test]
fn the_reach_of_a_run_is_every_later_unit_with_outputs() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/roles.lfy", STUBBED_ROLES)
        .write("def/c.lfy", "d C { $x = string; }\n")
        .write("def/b.lfy", "use \"./c\";\n\nd B { $c = C; }\n")
        .write("def/a.lfy", "use \"./b\";\n\nd A { $b = B; }\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    // The stub line of the target is the value its language's setter gives it.
    // @lfy def/generation/main.lfy:request#request:request:5367b0ce97e471a1ab266e29356a00115ec74e7fcc5ace4805721d77194a180e
    assert_eq!(
        stub_line_of(&workspace.model, &workspace.targets[0]).as_deref(),
        Some(STUB_LINE)
    );
    let maps = current_maps(&program, &plan_of(&program, &[], &[], &[]));
    let plan = plan_of(&program, &maps, &[], &[]);
    let stems: Vec<&str> = plan.units.iter().map(|unit| unit.stem.as_str()).collect();
    assert_eq!(stems, ["c", "b", "a"]);
    let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
    let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
    let existing = [Output {
        path: "src/b.rs".to_string(),
        text: "// old".to_string(),
    }];
    let request = request(&program, &plan, b_index, &existing, None, false, false, &[]);
    // @lfy def/generation/main.lfy:request#request:request:3fe3499800670559a48a3f228ba09e462416bfcd313e21d86ea7c91319ba2856
    assert_eq!(request.reach.units, [a_index]);
    assert_eq!(request.reach.fix_lines, 10);
    assert_eq!(request.reach.fix_unit_lines, 40);
    let text = &request.instructions;
    let reach = part(text, "## The reach of this run");
    // @lfy def/generation/main.lfy:request#request:request:7f048ee3afbc169385d5f7d6b40dbfba7314647d732caed0c4723e93ac727dd4
    assert!(reach.contains("- `a`: `src/a.rs`"), "{text}");
    assert!(!reach.contains("`c`"), "{text}");
    // @lfy def/generation/main.lfy:request#request:request:6866b21e4106d263e68cc05d262be33367c3a2efc454a318258613d378049040
    assert!(
        reach.contains("may be edited only as the part \"Stubs and fixes in later units\" says"),
        "{text}"
    );
    assert!(reach.contains("no other unit's code is edited"), "{text}");
    let stubs = part(text, "## Stubs and fixes in later units");
    // @lfy def/generation/main.lfy:request#request:request:23c0b1c1769d21fd81691a3e8adf218de21358861507f7e3ab4b0a8523d6ceaf
    assert!(
        stubs.contains(
            "edited only where this unit's own change would otherwise stop the target's build or \
             a test from passing"
        ),
        "{text}"
    );
    // @lfy def/generation/main.lfy:request#request:request:56cbf2c86de7cfc98b613cf1133778da57a587d2a252e62391857d01fa0dcc1e
    assert!(stubs.contains(STUB_LINE), "{text}");
    assert!(stubs.contains("keeps the item's signature"), "{text}");
    assert!(
        stubs.contains("write {{unit}} as the stem of the unit"),
        "{text}"
    );
    assert!(
        stubs.contains("{{reason}} as one line saying why the body was replaced"),
        "{text}"
    );
    // @lfy def/generation/main.lfy:request#request:request:5a3b2f10f5d312fabcd0e80c04eda116a66569db896d16d3c1df765c201a5b12
    assert!(
        stubs.contains("at most 10 lines in one region and at most 40 lines across the regions of one unit"),
        "{text}"
    );
    // @lfy def/generation/main.lfy:request#request:request:71a074ec9746e65337f38a647fd2376bc862cd1a25601b0650f2ffa6c566d5ec
    assert!(
        stubs.contains("An edit bigger than a fix may be is written as a stub."),
        "{text}"
    );
    // @lfy def/generation/main.lfy:request#request:request:00813b09739a9f5b779cac994285df76689c777d1752c796d019f6732b636c93
    assert!(
        stubs.contains("never changed, added, or removed"),
        "{text}"
    );
    // @lfy def/generation/main.lfy:request#request:request:e2b687435e285ea5c19b4f0a0d795cc0e9ab80185ea8f399f3b9e959041cdcda
    assert!(stubs.contains("A stub line left in the regions of the unit"), "{text}");
    assert!(stubs.contains("rejects the run"), "{text}");
}

/// The foreign edits the unit owns, in the order they were made, each with who made it,
/// its kind, where, its region's marker, its note, and its lines before and after.
// @lfy def/generation/main.lfy:request#request:request:3ad69cb145f65b3a03dbdf3d50854ec2941d4ce224b7946ca2f1b572362551d0
#[test]
fn a_request_gives_the_foreign_edits_the_unit_owns() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let maps = current_maps(&program, &plan_of(&program, &[], &[], &[]));
    let plan = plan_of(&program, &maps, &[], &[]);
    let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
    let stub = ForeignEdit {
        by: "b".to_string(),
        owner: "a".to_string(),
        path: "src/a.rs".to_string(),
        kind: ForeignEditKind::Stub,
        region: marker_of("def/a.lfy", "f", 3, 6),
        before: vec!["fn f() {".to_string(), "    old();".to_string(), "}".to_string()],
        after: vec![
            "fn f() {".to_string(),
            "    todo!(\"elfie stub: b: f's field changed type\")".to_string(),
            "}".to_string(),
        ],
        note: "f's field changed type".to_string(),
    };
    let elsewhere = ForeignEdit {
        owner: "c".to_string(),
        path: "src/c.rs".to_string(),
        kind: ForeignEditKind::Fix,
        ..stub.clone()
    };
    let existing = [Output {
        path: "src/a.rs".to_string(),
        text: "// old".to_string(),
    }];
    let request = request(
        &program,
        &plan,
        a_index,
        &existing,
        None,
        false,
        false,
        &[stub.clone(), elsewhere],
    );
    // @lfy def/generation/main.lfy:request#request:request:56b3e8ed8845880a5efc54ee766d63a577e1be64b2614849b89bca97e2254907
    assert_eq!(request.foreign_edits, [stub]);
    assert!(request.reach.units.is_empty(), "{:?}", request.reach);
    let text = &request.instructions;
    let edits = part(text, "## Foreign edits");
    // @lfy def/generation/main.lfy:request#request:request:d64715f05a3c70edd92983d1c91cc1002a00462c6f57ec320e5acb9580bc3cdb
    assert!(
        edits.contains("### 1. A stub by `b` in `src/a.rs`, in the region `// @lfy def/a.lfy:f`"),
        "{text}"
    );
    assert!(edits.contains("Note: f's field changed type"), "{text}");
    let before = edits.find("    old();").unwrap_or_else(|| panic!("{text}"));
    let after = edits
        .find("    todo!(\"elfie stub: b: f's field changed type\")")
        .unwrap_or_else(|| panic!("{text}"));
    assert!(before < after, "{text}");
    assert!(!edits.contains("src/c.rs"), "{text}");
    // @lfy def/generation/main.lfy:request#request:request:09e9aef14ec2e21a2597b994510795ad4658cd65c4665c53db6b26e6b2d31698
    assert!(
        edits.contains("written inside this unit's regions by the runs of the units they name"),
        "{text}"
    );
    assert!(
        edits.contains("Keep their code only where it does what this unit's criteria and tests ask"),
        "{text}"
    );
    // @lfy def/generation/main.lfy:request#request:request:29f2379bc0d7e987f6c443d1ffe113cf29d2293392d94b62ebfe545af3055eb0
    assert!(
        edits.contains("Replace each stub line in this unit's regions by code that does what"),
        "{text}"
    );
    // The foreign edits come after the unit's section and before the existing outputs.
    let at = text.find("## Foreign edits").unwrap();
    assert!(text.find("## The unit to compile").unwrap() < at, "{text}");
    assert!(at < text.find("## Existing outputs").unwrap(), "{text}");
}
