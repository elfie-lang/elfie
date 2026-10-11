# Roadmap: faster, server-first compiles

**Principle: push the obligations, pull the context, act through tools.**

- A request pushes only the obligations. These are the criterion ids and places, which of them are
  new, changed, or removed, the outputs, the rules, and the workflow.
- Everything else is pulled through the agent server: definitions, interfaces, code, and types.
- Agents act through tools whose input is checked when the call is made: reporting, marking,
  building, and testing.
- Command tools come from the target. A tool for an operation exists only when the target resolves
  a `Command` with a `line` for that `Operation`.

**Baseline** (2026-10-07/08 cycle):
- 24.8 agent-hours in total. Review took 14.1 h of it, at about 290k tokens of context per turn.
- The `generation/main` review request was 966k characters, and that batch took 10 reviews.
- Agents made about 800 raw Read/Grep/sed calls to find code.
- About 90% of the output tokens were hidden reasoning.

## Phase 1: the verifier on the agent server

0. **A compile-hosted agent server.** `elfie compile` serves MCP over HTTP on localhost.
   - It listens on a random port, takes a per-compile token, and lives as long as the compile.
   - It holds one session per agent run (each compiler or verifier run of a batch, and the global review), bound to that run's directory (its copy under `--jobs`),
     plan, request, and target. A tool acts only on its own batch.
   - The definitions stay as the compile planned them. The outputs are read fresh from the batch's
     directory.
   - The compiler and the verifier are each given a tool set.
   - The CLI writes an `--mcp-config` for each session.
   - The stdio `elfie mcp` stays for editors and interactive agents, with the same tool definitions.
1. **`elfie_report`.** The verifier submits each verdict by id or by place, and each one is
   checked as it arrives. The tool says what is still missing. The CLI reads the submitted
   verdicts and stops parsing the final message.
2. **A short review request.** It lists each criterion's id, place, and region locations, but not
   the code text. The verifier pulls code with `elfie_output <id>`.
3. **Incremental review.** Each verdict is stored with a hash of its region in `SourceMap`.
   - A review covers only criteria that are new, changed, last violated, or never reviewed.
   - `elfie verify --full` re-judges everything.

To measure, compare on `generation/main`: context per turn, review hours, lost verdicts, and rounds
until accepted.

## Phase 2: target commands as tools

4. The server offers `elfie_build`, `elfie_test`, `elfie_lint`, and `elfie_format` only for the
   operations whose resolved `Command` has a line. With several targets, a tool takes a `target`.
   - Each tool returns structured pass/fail and trimmed output.
   - Calls queue per directory in the compile-hosted server.
5. Commands may declare narrowing through the environment.
   - `TESTS` holds the filter words, and `SCOPE` holds the part of the outputs to run on (a
     package for cargo).
   - The rust vendor runs `set -f; cargo test ${SCOPE:+-p "$SCOPE"} -- $TESTS`, and build and
     lint take `SCOPE` the same way.
   - `elfie_build`, `elfie_test`, and `elfie_lint` take `scope`. `elfie_test` also takes `filter`.
   - *Deferred:* mapping a requirement id or entity name to the test functions marked for it.
6. Agents lose cargo `Bash` only when `ELFIE_COMMAND_TOOLS` names the tools their session is
   offered. The compiler keeps `cargo add`, which the ecosystem rule requires.

## Phase 3: the compiler on the agent server

7. `elfie_task` gives the obligations. These are the ids, each marked new, changed, or removed,
   with the text of the removed ones, plus the outputs and the reason.
8. `elfie_dependency` (`interface` is a reserved word) gives one dependency entity on demand, in place of the pushed interfaces
   section.
9. `elfie_output` gives generated code by entity or criterion id, with `text: false` for an outline of an
   output file.
10. `elfie_mark` sets or rewrites marker ids mechanically. `elfie_submit` gives a structured
    outcome in place of the `ELFIE:` lines.
11. Raw Read/Grep on outputs is denied once the code tools cover the need. Track the share of tool
    calls that go to the server.

## Phase 4: mechanical CLI steps

12. Marker ids of reworded criteria are rewritten by the CLI. `changes` already pairs them.
13. A unit planned only for `dependency` is re-recorded without a model when the target's build
    and test pass.

## Phase 5: scheduling

14. Dependents start on structural acceptance, and review runs alongside. Dependents are re-planned
    only if a retry changes the interface.
15. A rejected copy's outputs are kept as the next attempt's starting point.

## Phase 6: output layout and experiments

16. The 8–10k-line modules are split into one file per entity in the rust layout guidance.
17. Model tiering, and `claude -p --resume` for retries.

## Cross-cutting

- **Knowledge, not rediscovery.** Repository facts go in as knowledge items in `def/targets.lfy`,
  for example that the CLI crate is named `elfie`.
- **Keep the tool set small and general.** Every tool adds criteria to `mcp/main`.
- **Self-hosting safety.** The server that judges and acts runs the binary driving the compile,
  never output under review. A change to judging is landed through a patched bootstrap binary.

## Progress

- [x] Phase 1 (1.0–1.3), compiled in 8ce2f7b and validated on 2026-10-10:
  - mcp/data+1: 389 of 389 verdicts through elfie_report. Before, 4 of 389 survived the
    one-message report.
  - generation/main --full: the request was 166k characters (was 966k), took 14.9 min (was 20–35),
    and lost 0 verdicts.
  - A repeat verify after a clean compile carries every verdict and finishes in 9 s.
  - Open: context per turn rose to 389k tokens, because verifiers Read whole files. The prompt fix
    is in 9ad518e, and Phase 3's denial of raw reads is still pending.
- [x] Phases 2+3, compiled in ae52b1c and validated on 2026-10-10:
  - Each role is offered exactly its tools: 22 for the compiler, 18 for the verifier. elfie_format
    is absent, since this target's format command has no line.
  - elfie_test narrows with filter and scope, and long output is trimmed.
  - ELFIE_COMMAND_TOOLS gates Bash.
  - 100% of the compiler's tool calls went to the agent server.
  - The hosted request lists interfaces by name only.
  - Spent session configs are removed.
  - Fixed in 041b775: compiler sessions' own maps had empty hashes, and every session got the
    same instructions.
  - Open: a verifier still reads a single-entity 2.4k-line file whole. Item 16 (splitting
    modules) is the fix.
- [x] Phases 4, 5 and 6 (17 and knowledge), compiled in b89cb0d on 2026-10-10:
  - 7 of 7 accepted with 0 violated, and 735 verdicts carried.
  - Validated with a toy project of fake compilers and in scratch worktrees of this repository.
  - Passed:
    - dependency-only acceptance on build and test, with no compiler and no verifier;
    - early-starting dependents, including revalidation, and reruns from a new copy;
    - kept work, which resumes, is refused when the source changed, and is cleaned up;
    - session ids and the resume fallback;
    - quoted knowledge.
  - Open:
    - Reworded-only never fires for a committed reword. Previous-source recovery must match
      requirements too; this is a def fix, in progress.
    - The resume chain uses the first session even after a fresh one.
    - The retry line omits the dependency that changed.
    - The reworded-only path prints a duplicate accepted line.
    - Orphan kept files of unplanned targets are removed.
    - The last four are code defects, to be fixed by recompiling cli/main.
    - Maps recorded before b89cb0d lack SourceMap.text until re-recorded.
- [ ] Phase 6 item 16 (module split): guidance landed in 7b83be9.
  - generation/main's compiler built the split (21 files, all green) but had to revert it. accept
    checks a co-owner's markers per output path, and generation/data's map claims 225 regions in
    generation/mod.rs.
  - In progress: accept checks co-owner markers across outputs, and co-owner maps are re-derived
    after each recording.
