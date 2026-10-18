# Roadmap: unit compiles that always build

**Principle: one unit at a time, bottom up; the code builds after every unit; the root moves only
when the whole compile is done.**

## Why

Seen in the compile of 2026-10-17 (Configuration, `File.name`, `typeTextOf`, list item types):

- `workspace/data` blocked: its new required fields broke struct literals in units compiled later,
  and the gate says the whole workspace builds after every batch.
- `interpret/main` was rejected twice because the crate did not build, through code in
  `workspace/load.rs` that only `workspace/main`, a later batch, could regenerate.
- `workspace/main` was reviewed clean and then discarded, because units it depends on were rejected.
- A unit's request depends on which other units happen to be stale with it, so the same definition
  can be compiled from different prompts.

## Guarantees

- **G1 Order.** Units run one at a time, in `Plan.units` order. The order comes from the plan
  alone.
- **G2 Requests.** A unit's request is built from its definitions, the interfaces of its
  dependencies, the target, and the current text of its outputs. Every edit another unit made to
  those outputs is listed in it. A request never depends on which other units are planned.
- **G3 Always builds.** After every accepted unit, the target's build passes for the whole
  workspace.
- **G4 Always tests.** After every accepted unit, every test passes or fails only by reaching a
  stub.
- **G5 Bounded reach.** A unit edits another unit's outputs only inside that unit's regions, only
  for a unit later in plan order, and only as a stub or as a fix under a size cap.
- **G6 Finishes.** Every stub sits in a later unit's code, and that unit cannot be accepted while it
  holds one, so a bottom-up compile ends.
- **G7 Root.** The root takes every unit of the compile at once, and only when no stub is left, the
  whole build, test, lint, and format check pass, every planned unit is accepted and reviewed, and
  the global review passes.

G2 holds for the definitions, not for the code. A touched unit's request carries code that an
earlier LLM run wrote. That is already true of `Request.existing` today; the edits are now listed,
named by the unit that made them.

## Terms

- **Change set:** every planned unit of one compile for one target, worked in one copy of the root.
- **Foreign edit:** an edit a unit makes inside a region of another unit's output.
- **Stub:** a foreign edit that only removes lines and adds the target's stub line. It is the
  fallback that always builds.
- **Fix:** any other foreign edit, capped in size.
- **Touched:** the reason a unit is planned when another unit made a foreign edit to its outputs.

## Phase 0: finish the current compile

1. Let the `--no-verify` compile finish, run `elfie verify`, and recompile what it finds violated.
2. Switch `elfie.json` dependencies to strings (`"elfie": "./lib"`), rebuild, test, and commit.
3. Keep the knowledge note "A change that breaks a dependent's output" until Phase 4 lands, then
   remove it.

## Phase 1: targets say how to stub

`lib/target/language.lfy`, `lib/criteria/command.lfy`, `vendor/rust/main.lfy`. No unit changes.

4. **`targetLanguage` gains two members:**
   - `stubLine`: the statement that replaces a body. Its `{{unit}}` and `{{reason}}` are filled in.
     For Rust it is `todo!("elfie stub: {{unit}}: {{reason}}")`.
   - `stubMarker`: the text every stub line holds and every stub prints when it is reached. For
     Rust it is `elfie stub:`.
5. **Language criteria:**
   - A body replaced by the stub line keeps its signature and builds.
   - Reaching a stub fails, with a message that holds the stub marker.
6. **Commands:** a test command reads `ALLOW_STUBS`.
   - When it is set, the line exits 0 when every failing test failed by reaching a stub.
   - For Rust that is a small script in `vendor/rust` that reads `cargo test`'s per-test output.
   - Without it, the line behaves as it does now.
7. **Rust guidance:** a struct that derives `Default` is built outside its own unit with
   `..Default::default()` after the fields it sets. Adding a field then breaks no dependent, so most
   data changes need no foreign edit at all. It does not help a field whose type has no `Default`,
   or a removed or renamed field.

## Phase 2: records

`def/generation/data.lfy`, `def/cli/data.lfy`, `def/mcp/data.lfy`.

8. **Remove the batch:** `Batch`, `batchUnits`, `batchCharacters`, and `Plan.batches` go.
   `Plan.units` is the schedule.
9. **`Reason` gains `touched`:** another unit edited its outputs during this compile.
10. **`d ForeignEdit`:**
    - `by` and `owner` (units), `path`, `kind` (`stub` or `fix`), and the region's lines before and
      after.
    - `note`: why, from the stub line or from `elfie_touch`.
11. **`Request` is for one unit:**
    - `unit` replaces `batch`, and the maps keyed by path collapse to the one unit.
    - New `foreignEdits`: the edits other units made to its outputs since it was last accepted.
    - New `reach`: the later units it may edit, their outputs, and the fix cap.
12. **`ReviewRequest`, `Outcome`, `Binding` and `compilerTool` name one unit.**
    - `Outcome.verdicts` becomes a single `Verdict`.
    - The `Verdict` lists the foreign edits it made.
13. **`d ChangeSet`** (`cli/data`):
    - the units in order, the copy's directory, each unit's state, and the foreign edits recorded;
    - kept in `elfie-compile/cache` so a compile that stops can resume.

## Phase 3: planning, requests, and acceptance

`def/generation/main.lfy`.

14. **`plan`:** drop the batching criteria and their tests. Order, moves, and reasons stay.
    - A unit with a recorded foreign edit is planned with the reason `touched`, unless it already has
      a reason.
    - A touched unit is always later in plan order than the unit that touched it.
15. **`request`:** one unit.
    - The instructions put what is the same for every unit of a target first, so a prompt cache can
      reuse it: role, guidance, native dependencies, knowledge, commands.
    - Then interfaces, then the unit, its foreign edits, and its existing outputs.
    - This is how the shared context batches existed for is recovered.
16. **`accept`:** classify every changed line outside the unit's own regions.
    - **Shared outputs:** as today (`lib.rs`, `mod.rs` lines).
    - **In another unit's region:** rejected unless that unit is later in plan order and the
      region's markers are unchanged.
      - A **stub** if the diff only removes lines and adds stub lines naming this unit.
      - Otherwise a **fix**, rejected over `fixLines` changed lines per region or `fixUnitLines` per
        touched unit (start at 10 and 40, and tune them from measurements).
      - Every touched owner needs a note (`elfie_touch`) for its fixes.
    - **In no unit's region and not a shared output:** rejected, as today.
17. **Recording:** the foreign edits go into the change set.
    - The owner's region hashes no longer match, so its earlier review verdicts no longer count (as
      today).
18. **Touched-fix path, no compiler:**
    - Applies when a unit's only reason is `touched` and all its foreign edits are fixes.
    - It runs the build and tests, then the verifier on the touched entities only, and is accepted
      if both pass, as the dependency-only path is now.
    - A stub always goes to the compiler.

## Phase 4: the compile loop

`def/cli/main.lfy`, `def/targets.lfy`.

19. **One copy per target per compile** holds the change set.
    - It is kept in `elfie-compile/cache` when the compile stops.
    - The next compile resumes it from the first unit not accepted, provided the definitions of the
      accepted units are unchanged.
20. **Each unit, in order:**
    1. The steps that need no compiler: reworded-only, dependency build-and-test, touched-fix.
    2. Otherwise, a compiler run.
    3. `accept`.
    4. The unit gate: the whole workspace builds, tests pass with `ALLOW_STUBS`, and lint passes on
       its own outputs.
    5. The verifier on its own criteria, with earlier verdicts carried.
    6. The next unit.

    Retry counts stay per unit, as they are per batch today.
21. **A unit rejected after its retries stops the change set.**
    - Nothing merges.
    - With `--continue`, units that are not above it still run, to surface their problems, but they
      merge only with the rest.
22. **Final gate**, before the root takes the change set:
    1. No output line is a stub line, read as accept reads one: a whole line, so a test that quotes
       the marker inside a string never matches.
    2. Build, test without `ALLOW_STUBS`, lint, and format check pass for the whole workspace.
    3. Every planned unit is accepted and reviewed, none violated.
    4. The global review passes.

    Then the copy is merged into the root at once, and maps and review records are written.
23. **`def/targets.lfy`:** the gate criterion becomes two.
    - After every unit, the workspace builds and every test passes or fails only at a stub.
    - At the end of a compile, `cargo build`, `test`, and `clippy` pass without warnings.
24. **Progress:**
    - Batch steps and lines go.
    - Each unit gets a line.
    - New steps: `touched`, `stubbed`, `finalGate`, `merging`.
25. **`--jobs` goes** until Phase 6. Running one unit at a time is what makes the order
    deterministic.

## Phase 5: the agent server

`def/mcp/main.lfy`, `def/mcp/traits.lfy`, `scripts/compile-unit.sh`, `scripts/verify-unit.sh`.

26. **The tools take a unit, not a batch:**
    - `elfie_units` lists units and reasons, with no batches.
    - `elfie_request` and `elfie_review` take a unit.
    - `elfie_task` loses its unit argument.
27. **`elfie_reach`:** the later units this run may edit, their regions, and what is left of the fix
    cap.
28. **`elfie_touch(unit, note)`:** says why this run edited a unit. Accept requires one note per
    owner of a fix.
29. **`elfie_test` gains `allowStubs`**, passed as `ALLOW_STUBS`.
30. **The scripts:** `ELFIE_BATCH` and `ELFIE_UNITS` become `ELFIE_UNIT`.

## Phase 6: parallel units

Superseded by Phase 9: "no unit depends on both" allows no parallel pair in this repository,
because `cli/main` depends on every other unit.

## Phase 7: records and cleanup

32. **Record the decisions:**
    - Add a `def/DECISIONS.md` entry.
    - Remove the knowledge note from Phase 0.
    - Update the README's description of a compile.
33. **Remove dead parts:** the parallel-copy rules for batches, kept work per batch, and the batch
    identifiers in progress and in requests.

## Rules for Phases 8 to 11 (2026-10-18)

- **Express every change as acceptance criteria, tests, members, parameters, or other code-level
  data.** Add no `// Decision:` comment to any `.lfy` file, and add nothing to `def/DECISIONS.md`.
  Delete a `// Decision:` comment that a change makes false; move an existing one along with the
  entity it sits on. Rationale goes in commit messages.
- **Stage 1 is Phases 8 to 10,** compiled by today's binary. **Stage 2 is Phase 11,** compiled by
  Stage 1's binary, because the split needs Phase 10's relocation already built.

## Phase 8: request time

Measured 2026-10-18: a request took 3–6.5 minutes because the accepted source was searched for by
lowering the whole program once per git revision of the unit's file (about 3.9 s each), twice per
request, and found nowhere when it was never committed.

34. **One recovery of the previous source, shared.** `d Previous` in `generation/data`:
    - `text`: the file as it was;
    - `program`: the program lowered with the unit's file read as that text;
    - `hashMatched`: whether it is only the hash-matched source.

    `fn previousOf(program: Program, unit: Unit, sourceMaps: SourceMap[]) => Previous | undefined`
    goes in a new `def/generation/previous.lfy`, which uses the loader, the interpreter, and git.
    The recovery criteria now duplicated in `cli/main` (about lines 1145–1180) and in `mcp/main`'s
    `changes` tool (about lines 1140–1160) move into it, and those two places call it.
35. **The working tree first.** When the unit's file as loaded lowers to `SourceMap.hash` of the
    unit's outputs and its local ids hash to `SourceMap.requirements`, it is the source at the last
    accepted generation. `Previous.program` is then the `program` given, and git is not run.
    - Otherwise each git revision is lowered at most once, newest first, as today.
36. **Recovered once per unit per compile.** The compile calls `previousOf` once for a unit and
    hands the same `Previous` to the rewording, to `request`, to `reworded`, and to the checks of a
    kept unit.
37. **The previous file lowered once.** `request`, `changes`, and `reworded` take a `Previous`
    instead of previous text and read the file as it was from `Previous.program`. They lower and
    bind nothing. `request` loses its `hashMatched` parameter, which `Previous.hashMatched` replaces.
38. **No loader in `generation/main`.**
    - Its only uses were `Loader.change`, which item 37 removes, and two knowledge criteria that
      name "the source path `Loader.load` gives". Restate those from data: the `Package.root` of the
      package of the file its contributor is declared in, or `Workspace.root` for the project's own
      files, joined with `Knowledge.source`.
    - Remove `use "../workspace/main"` from `generation/main`.
39. **`change` reuses what is already parsed** (`def/workspace/main.lfy`).
    - Every file other than `path` keeps the `File.tree` it has in the workspace given, and only
      `path` is read from disk.
    - The criterion "the result is what `load` of the same root gives" holds when every other file
      holds what it held when that workspace was loaded.

## Phase 9: start a unit as soon as what it waits for is merged

40. **`Unit.waitsFor`** (`generation/data`, set by `plan`) lists the stems of the planned units it
    starts after, in `Plan.units` order. A planned unit is listed when it is one of the unit's
    `Unit.dependencies` and either:
    - it has no recorded outputs; or
    - the SHA-256 of its `interfaceOf` now differs from what the unit's outputs record in
      `SourceMap.dependencies`.

    This is decided from definitions alone, before anything runs.
41. **Reach is only the units that depend on it.** `Reach.units` is every unit of the same target
    with outputs that depends on the unit, directly or through other units. Delete the
    Decision comment above `d Reach` that says otherwise.
    - A unit whose `interfaceOf` hash is what every one of its dependents' outputs record makes no
      foreign edit; `accept` rejects one.
42. **Scheduling** (`def/cli/main.lfy`): replace "Units run one at a time" and "A unit starts only
    once every planned unit before it is done".
    - **When a unit starts:** once every unit in its `waitsFor` is merged, skipped, or stopped. At
      most `--jobs` units run at once (default 4, at least 1), and among the units ready to start,
      the earliest in `Plan.units` starts first.
    - **Its copy:** a copy of the change set's base with the merged outputs, maps, and review
      records of every unit it waits for, directly or through those, written in. Nothing else is
      in it, so the copy does not depend on timing.
    - **Merging:** a unit that finishes merges into the change set once every planned unit before it
      in `Plan.units` has merged, been skipped, or stopped. It writes its own outputs, maps, review
      records, and foreign edits.
    - **After each merge,** the change set runs the target's build, and its test with stubs
      allowed. If either fails, or if a region the unit wrote was changed by a unit merged after its
      copy was made, the merge is undone and the unit runs again in a new copy of the change set as
      it stands. That run does not count toward its retries; a second such run in a row stops it.
    - **Kept as they are:** the final gate, all-at-once merge into the root, `--continue`, and
      resuming.
    - Reword the progress line rules (`Step`, `Progress`) that assume one unit at a time.
    - `--jobs` comes back with this meaning.

## Phase 10: relocating code when an entity moves to another file

43. **Before planning, in `compile` and `verify`,** a step that needs no compiler relocates code.
    For each marker in a target's outputs whose `Marker.file` is a project file that no longer
    declares the marker's entity (or that member's owner):
    - The marker is rewritten to name the other file when exactly one other project file declares
      an entity of that name, of the same kind, built for the same target. Ids are unchanged; they
      hold no path.
    - The source maps of both units are re-derived with `rederive`.
    - The review records of the relocated entities' ids move from the old unit's record file to the
      new unit's, so their verdicts carry.
    - With no file, or more than one, declaring it, nothing is rewritten and the unit stays as
      today.

    Put the function in generation (next to `rederive` and `rename`) and call it from `cli/main`.
44. **The request says so.** The new unit's request lists the entities that were relocated into it
    and the file each came from, and says their code is kept as it is (`Request.relocated` in
    `generation/data`, rendered like "entities moved into"). The old unit's request lists them as
    moved out.

## Phase 11: split the largest units

Compiled by Stage 1's binary, so Phase 10 moves the code instead of regenerating it.

45. **`def/generation/main.lfy`** keeps `interfaceOf` and `plan`. The other functions move, using
    the grouping already drafted in `~/elfie/def/generation`:
    - `maps.lfy`: `sourceMapsOf`, `record`, `regionsOf`.
    - `request.lfy`: `request`.
    - `changes.lfy`: `changes`, `reworded`.
    - `accept.lfy`: `accept`, `rederive`, `rename`, and the relocation step.
    - `outcome.lfy`: `outcomeOf`.
    - `review.lfy`: `review`, `globalReview`, `reviewsOf`, `recordReviews`.
    - `findings.lfy`: `reviewOf`.
    - `previous.lfy`: from Phase 8.

    Unlike `~/elfie`, **record types stay in `generation/data`.** Moving them next to their
    functions would make `cli/data`, `mcp/data`, and `cli/style`, which use them, depend on
    behaviour units and climb the chain. Each `ace const` moves with the function that reads it.
    Every `[[...]]` reference in other files is updated to the entity's new module, and every
    `use` is narrowed to what the file reads.
46. **`def/mcp/main.lfy`** keeps `serve` and `host`. Its tools move by family:
    - `mcp/read.lfy`: the tools that read the program and outputs, which need query and the maps;
    - `mcp/compiling.lfy`: the tools that carry `compilerTool`, plus `report` and `reportMany`;
    - `mcp/commands.lfy`: the tools that carry `commandTool`.

    Rewrite criteria that say "every fn of this file that carries [[tool]]" to name the tool files,
    or the trait's applications, so the tool set is unchanged.
47. **`def/cli/main.lfy`:** split `main` by command into functions in separate files, and have
    `main` dispatch to them. Keep each command's criteria and tests with its function, and keep
    `parse` and dispatch in `cli/main`.
    - `cli/inspect.lfy`: `check`, `format`, `tree`, `tokens`, which need only the loader,
      formatter, lexer, parser, and query;
    - `cli/compile.lfy`: `compile` and `verify`, which need generation and the agent server's host;
    - `cli/main.lfy`: `help`, `version`, `init`, `lsp`, `mcp`, and everything shared, such as
      color, progress, environment, and agent sessions, unless it belongs to one command only.
48. **Confirm the depth.** Afterwards `elfie compile --dry-run` and the dependency levels show the
    split. `generation/main` no longer depends on `workspace/main`, and no new unit sits above
    `cli/main`.

## Compiling the shift

- **Phases 2 to 5 are one definitions change,** compiled by today's batch binary.
- It changes `generation/data`, `cli/data`, and `mcp/data`, so it hits the same breakage this
  roadmap removes. Run it with `--no-verify`, then `elfie verify`, as in Phase 0.
- Phase 1 changes only `lib/` and `vendor/` and can go first.
- After Phase 4, the new binary compiles everything after it, which is the first real test.

## Measurements

Against the 2026-10-17 compile:

- **Wall time, compiler runs, and verifier runs.** Expect more runs than with batches until Phase
  6, but fewer retries.
- **Rejections caused by another unit's code:** target 0.
- **Units discarded after a clean review:** target 0.
- **Foreign edits:** how many stubs and fixes per compile, and how many touched units the
  touched-fix path settled without a compiler.
- **Prompt cache reads** on the stable prefix of unit requests.

## Decisions (2026-10-17)

- **Fix caps:** `fixLines` is 10 changed lines per region and `fixUnitLines` is 40 per touched
  unit. Anything bigger must be a stub.
- **`--continue` stays** with the meaning in item 21: units not above a failed one still run, and
  nothing merges without the rest.
- **The `..Default::default()` rule (item 7) is adopted** as a guidance criterion of the Rust layer
  in `vendor/rust`, so every Rust project gets it. Data structs derive `Default` where their field
  types allow it.
- **A kept change set** is resumed while the definitions of its accepted units are unchanged; a unit
  whose definitions changed, and every unit after it, runs again.
- **Commits:** one local commit on `agentic-main` after each phase passes its checks. Nothing is
  pushed.
- **Confirmed 2026-10-18,** from the first compiles with the new binary: the final gate looks for
  stub lines the way accept reads them, not for any line holding the marker; and a unit a kept
  change set accepted stays planned, so a compile of units requested by name can resume.
