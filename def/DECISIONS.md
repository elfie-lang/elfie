# Decisions from scoping generation, LSP, MCP, and CLI (2026-09-18)

Everything below is exploration. Each entry says what was chosen, what the alternative was, and why
the choice is the recommended one. Items marked **Issue** need a decision or a fix from the owner.

## Layout

New modules, in the order they were written:

| Module | Purpose | Depends on |
| --- | --- | --- |
| `def/query/` | One query surface over a bound `Workspace`: positions, symbols, hover, completions, diagnostics, outline, rename, `find`, `sourceOf` | model, parser, lexer, workspace |
| `def/format/` | `format(tree) => string`, the standard layout | grammar, parser |
| `def/generation/` | `plan`, `request`, `accept`: units of work, the prompt, and structural acceptance | model, workspace |
| `def/lsp/` | The language server: lifecycle, documents, feature-to-query mapping | query, format, workspace |
| `def/mcp/` | The agent server: one tool per `fn is tool` | query, generation, grammar, workspace |
| `def/cli/` | `parse` and `main`: every command as a thin driver | every stage |

**Chosen:** a separate `def/query/` module rather than putting the queries in `def/lsp/` and having
MCP import them. The request says MCP is "the same query surface exposed to agents rather than
editors"; that is only literally true if the surface is a module of its own that neither server owns.
It also keeps `elfie-mcp` from depending on `elfie-lsp`. Query belongs in the pure core crate with the
model: it is functions over `Workspace` and nothing protocol-shaped.

**Chosen:** a separate `def/format/` module because both the CLI and the LSP format and neither
should own the rules. The formatter is small and was not asked for, but `elfie format` was, and a
formatter without rules is not a definition.

## Generation

**Unit = one source file for one target** (`Unit`). Alternatives: one entity per unit (rejected:
outputs and source maps are per file, and the compiler would have to merge into a file it does not
own, which is itself generation with no criteria), or the whole target per unit (rejected: does not
fit a request, and loses incrementality).

**Selection of what is built.** Workspace said "the entities built for a target are `entitiesOf`
the marker". Nothing in this repository carries a target marker, and marking every declaration
`is rust` is not a practical way to build the compiler itself. Chosen: an entity is built when it
carries the marker, when its file's anonymous entity carries it, or when `global` carries it.
`def/workspace/main.lfy` line 73 was edited to say the same so the two stay consistent.
**Issue:** how a marker is applied to a file entity or to `global` in source is not specified
(`rust.apply(@)` at file level, or `rust.apply(global)`, are the obvious spellings; the model does
not say whether `@` with no name in a file scope resolves to the file entity for `apply`).

**Package files give no units.** A target package or dependency is compiled by its own project.

**Staleness** (`Unit.reason`): `requested` > `fresh` > `changed` > `dependency` > up to date.
Change detection is a SHA-256 of the source bytes recorded in the `SourceMap` at acceptance. The
alternative, diffing against git, was kept only as the way to recover `Request.previous` (the source
at last generation) because it is not always available; a hash is. **Chosen:** one
`source-map.json` per target under its output directory rather than one per output directory. The
per-directory maps in earlier hand compiles were an artifact of compiling one stage at a time.

**The request carries everything resolved** (`Request.instructions` quotes every criterion and test
via `criteriaOf`), and the MCP server is offered for what is left out. Alternative: a lean prompt
that relies on MCP for everything. Rejected because a compiler that cannot reach the server must
still work, and because a quoted criterion is deterministic while a tool call sequence is not.

**Ambiguity is a compile error, never a guess.** This is the owner's standing rule and is now a
line of every request.

**`accept` is structural only:** paths under the output directory, markers naming the unit's file
and real lines, one marker per entity. Whether the output builds or its tests pass is for the
target's guidance and the CLI's compiler command. Nothing here shells out.

**Marker format** is fixed here (`@lfy path:line[:column]` after the target's line comment opener)
because the source map reader must parse it independent of target.

## LSP

**The whole server is compiled from `def/lsp/`,** with the protocol library declared `external`.
This reverses the earlier note that plumbing stays hand-written; that note predates `external`
declarations and native dependencies. A crate must be all compiled or all written, so the plumbing
is defined too.

**Full text synchronization,** because `Workspace.change` takes whole text; incremental sync would
only be reassembled into it. **Push diagnostics** (publish after each change, latest wins), not
pull. **Position encoding** utf-8 when offered, utf-16 otherwise; queries never convert, the server
does at its edge.

**Not included:** semantic tokens (a TextMate grammar covers highlighting for now), code actions,
signature help, inlay hints. Each is one more query when wanted.

## MCP

**Tools are `fn ... is tool`** and the tool list is the context layer of `def/mcp/main.lfy`, never a
second copy. Argument descriptions come from `///` documentation lines on the fn because a
`Parameter` has no place for a description (see Issues).

**The server never writes.** During compilation the agent writes outputs itself and calls
`elfie_check`; the CLI records source maps. One writer keeps one path for "what is on disk".

**Tools:** `problems`, `find`, `entity`, `references`, `outline`, `grammar`, `units`, `request`,
`check`. No resources, no prompts: one surface is enough to keep exact.

**Freshness:** before each call, files whose mtime or size changed are re-read through
`Workspace.change`. Alternative: a file watcher (rejected for now: more moving parts than a stat).

## CLI

Commands: `help`, `version`, `init`, `check`, `format`, `tree`, `tokens`, `compile`, `lsp`, `mcp`.
`--tree <file>` is kept as an alias for `tree` so what shipped keeps working. Exit codes:
0 success, 1 problems, 2 usage, 3 failure.

**`compile` drives an external compiler command** named under `compiler` in `elfie.json` (for
example a `claude -p` invocation with an MCP config pointing at `elfie mcp`). The request goes on
stdin; the root is the working directory; `ELFIE_ROOT` and `ELFIE_UNIT` are set. Alternatives:
embed an LLM client in the CLI (rejected: couples the compiler to one vendor and one harness), or
only emit request files (kept as the fallback when no compiler is configured). One retry with the
rejection problems appended, then stop. **Assumption:** the `compiler` key in `elfie.json` is new
and the workspace stage does not read it; the CLI reads it directly.

## Edits to the owner's WIP files

- `def/workspace/data.lfy`: added `Workspace.nativeDependencies`. The project's own native
  dependencies (for example `tower-lsp`, an MCP crate, `clap`) had nowhere to live; only packages
  had them.
- `def/workspace/main.lfy`: one criterion for the new member, and the target selection criterion
  widened as described under Generation.

## Issues found

1. **Parameters cannot carry a description.** `Parameter` is `Name ? DefinitionClause`, and the
   `DefinitionClause` holds the type. There is no way to write `file: string` *and* describe `file`.
   MCP works around it with `///` lines; a language fix (a description after the type, as data
   members have) would be better.
2. **No spelling for applying a marker to a file or to `global`.** See Generation.
3. **`Entity.sideEffects` still has no criterion saying what writes it** (open from the model
   round).
4. **`[[` inside code** lexes as two `ListOpen` tokens (correct: `ReferenceOpen` is a candidate
   only in templates and documentation), but `scripts/refcheck.py` reads it as a reference. The CLI
   tests are written `[ [ ... ] ]` to keep the checker clean. Either the checker or the convention
   should change.
5. **Two `Session` data declarations** exist, in `def/lsp/data.lfy` and `def/mcp/data.lfy`. Legal,
   since neither is a rule, but a compiler mirroring the def layout into one crate would clash.
   They are different things; renaming one to `Connection` is the cheap fix if it bites.
6. **MCP protocol revision** is pinned to 2025-06-18 in the `external` declaration. Pin whatever the
   chosen library supports.

## Verification

`scripts/check.sh` parses every file under `def/` with the built parser from `main` and runs
`scripts/refcheck.py`. As of this writing all 38 files parse and the checker reports zero of each
problem kind.
