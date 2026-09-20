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

**Semantic tokens** were added on 2026-09-19 (`semanticTokensOf` in `def/query`, one feature in
`def/lsp`): only names are classified, keywords and literals stay with the editor's grammar, the
three layers become modifiers, and `data` and `trait` are custom token types the extension maps
to scopes. **Not included:** code actions, signature help, inlay hints.

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

# Decisions from compiling `def/` to Rust (2026-09-18, second round)

## Crates

`Cargo.toml` is a workspace: `crates/elfie-core` (grammar, lexer, parser, model, workspace,
query, format, generation), `crates/elfie-lsp`, `crates/elfie-mcp`, `crates/elfie-cli` (package
`elfie`, binary `elfie`). The old `src/` moved to `crates/elfie-core/src/` unchanged except for
`@lfy` markers re-pointed at the lines the defs now have. One target, `rust`, declared in
`targets/rust/main.lfy` and applied to `global`; its guidance says which crate a stem lands in.
`elfie.json` gained `dependencies`, `targets`, and `native` in the shape the workspace loader reads.

## The binder (model)

- **Trait bodies run at bind time, per application.** Applying a trait executes its body with the
  parameters bound to the evaluated arguments and `@`/`$`/`.` meaning the receiver: members join
  the receiver's scope, `.x = v` setters land in `Entity.values`, `where` and `add` calls append
  criteria with the trait as contributor, `if`/`match`/`for`/`with` execute. Extended traits are
  applied first, with their arguments evaluated from the extending trait's parameters. This is what
  makes `rule@entities`, `binding.apply(., operator.precedence, ...)`, `{{terminals}}` (the whole
  EBNF document, produced by running `terminalDocument()`), and `for (const rule in
  terminal@entities) { where ... }` come out right. The interpreter lives in `model/eval.rs`.
- **A trait's own body also runs, dry:** criteria and tests attach to the trait itself (so a target
  marker's guidance is `criteria_of(marker)`), but applies, setters, and `with` blocks are skipped,
  and an unbound `{{param}}` renders as written.
- **`with X` inside a trait body** redirects where criteria attach but keeps `@` on the receiver, so
  `trait scoped { with Scope { where (\`A [[Node]] for {{@identifier}} is bound\`) ... } }` puts one
  criterion per scoped rule on `Scope`. Outside a trait, `with X` makes X current.
- **Template references render as written** (`[[Token.file]]` stays `Token.file`) unless the head
  is a variable holding an entity (`[[&rule]]` becomes `Space`). `criteria_of` strips the brackets;
  evaluated template values keep them because EBNF needs them.
- **Members may share a name with a parameter** (`trait binding(precedence) { $precedence = ... }`);
  every other double declaration in one scope is a problem.
- **Member lookup is lenient** where the left side's type is a trait predicate, a union, or unknown
  (`rule.lexCondition`, `tree.tokens.get`); it is strict for modules, data, enums, and types.
- **Function bodies** (`function`) are not executed at bind time except for their context statements
  (`@acceptanceCriteria`, `@test`, `where`, `with`, `for`); they are executed as code only when
  called from a template or trait argument (`Grammar.grammarDocument()`).
- **Node identity** is `(file, preorder index)` with a path back to the node; a node is also found
  by `(file, start, end, rule)`, which is unique because nested nodes with the same span differ in
  rule.

## Def defects found by the binder (fixed in place)

- `def/parser/components.lfy` used `ReferenceClose`/`ExecutionClose` without
  `use "../grammar/terminals/literal"`; `scripts/refcheck.py` had hidden it with its global rule
  fallback, which the model only grants inside templates.
- `def/model/main.lfy` referred to `Entity.entities` twice; it is `TraitEntity.entities`.
- `def/grammar/traits.lfy` line 87 referenced `[[item]]`, which nothing declares; reworded.

## Known issues left open

- A reported parse failure for `x = c ?? d;` turned out not to be a bug: `d` is the data
  keyword and can never be a name, so the sample itself was invalid; `x = c ?? e;` parses. The
  error the parser gives (`expected [Semicolon] but found "= d"`) does not say so, which is the
  real shortcoming; a keyword-in-name-position diagnostic would need a criterion in the parser
  def before the compiled parser can grow one.
- The formatter treats a trailing comma as layout (added on wrap, removed on one line), which is
  the one deliberate deviation from "the same tokens, trivia aside".

## Self-compilation, as it stands

`elfie check` binds the 39 files (38 definitions plus the target package) with no errors;
`elfie compile --dry-run` plans 36 units in dependency order; `elfie compile --accept --all`
checked the crates on disk against every unit and recorded `crates/source-map.json` (92 maps,
3779 markers), after which every unit is up to date. `elfie lsp` answers hover, definition,
symbols, completion, and diagnostics for the definitions; `elfie mcp` serves the nine tools, and
`elfie_entity Token.raw` reports the member's definition, type, eleven references, and source.

Decisions made on the way:

- **`compile --accept`** was added to `def/cli/main.lfy`. The def only recorded source maps
  after running the compiler command, which left no way to record work done through the agent
  server (the intended workflow: the agent writes, `elfie_check` verifies, the CLI records). The
  compiler that produced these crates was this session, working exactly that way.
- **An output may carry markers for several definition files.** `accept` originally rejected any
  marker naming a file other than the unit's; real outputs (`lib.rs` mirroring `mcp/main`,
  `mcp/data`, and `mcp/traits`; `parser/data.rs` marking a field from `parser/traits.lfy`) do that
  legitimately. Now only a marker naming a file outside the program, or a line past the unit's
  file's end, is a problem, and a file shared by several units is an output of each.
- **Markers are per criterion, not only per declaration.** The compiled code marks the line of
  the criterion each block satisfies; acceptance only requires one marker inside each entity's
  declaration, so both styles pass.
- **`exit` in the language server ends on stdin EOF.** tower-lsp ends its loop when the client
  closes the pipe; editors do that after `exit`. A shutdown request must be sent without a
  `params` member, as the protocol specifies.
- **Unused-`use` warnings** are reported by `check` for nine `use`s in `def/grammar/main.lfy` and
  a few elsewhere whose purpose is to load files, not names. They are warnings, not errors, and
  the criterion in `def/query/main.lfy` may want an exemption for a file that declares nothing.

## Claude Code as the compiler (2026-09-19)

`elfie.json` names `scripts/compile-unit.sh` as the compiler: `claude -p` in `acceptEdits`
mode, allowed to read, edit, glob, grep, run `cargo build/test/clippy/fmt`, and call the `elfie`
MCP tools, with `scripts/compiler-prompt.md` appended to its system prompt. The request text
is its prompt on stdin. `.mcp.json` registers the agent server for every Claude Code session in
the repository through `scripts/elfie-mcp.sh`, which prefers a built binary over `cargo run` so
the server never contends for the build lock. Decisions: a wrapper script rather than a long
command in JSON, so the flags are reviewable and editable; `acceptEdits` rather than
`bypassPermissions`, so anything outside the allowed list still stops the run; no `--bare`, so
the repository's own settings and CLAUDE.md apply.

# Merge of main and the compile-process changes (2026-09-19)

Main's `def/` (grammar, lexer, parser, model, workspace, query) and its compiled core modules
replaced this branch's copies; the query Rust stayed this branch's, since main's is a stub. The
unwritten pieces (cli, format, generation, lsp, mcp) were formatted with `elfie format` to match.

- **Markers name entities, not lines.** `@lfy def/lexer/main.lfy:lex` (or `Entity.member`).
  Lines are derived from the model at acceptance and a line-form marker that falls inside a
  declaration is rewritten to the name form, so a source map never contains a number the compiler
  typed and no marker drifts when definitions move. Alternative rejected: keeping line markers
  and asking the compiler to shift them, which is exactly the error-prone step this removes.
- **Interface signatures decide dependency staleness.** A source map records the hash of the
  unit's interface text (identifier, kind, definition, type, parameters, output per entity) and
  the hashes of its dependencies' interfaces. A dependent is planned only when one of those
  differs, so a change inside a body regenerates one unit. Timestamps are no longer consulted.
- **Batches.** Planned units are grouped by plan order and first stem segment, at most six units
  or 60,000 characters of source per batch, with a dependency-only unit joining the batch that
  produces what it depends on. One request per batch sends guidance and interfaces once.
- **Outcomes.** The compiler ends its report with `ELFIE: DONE`, `ELFIE: BLOCKED: <why>`, or
  `ELFIE: CLARIFY: <question>`; `outcomeOf` reads that line and the verdicts. Rejected batches are
  retried once with the problems; blocked and clarification stop the compile and leave a file
  under `elfie-requests/` for the person; failures retry once. `--continue` lets independent
  batches proceed past a stopped one.
- **Progress** is one line per step (planned, requesting, compiling, checking, accepted, ...) with
  done/total and elapsed seconds, as JSON with `--json`, with the compiler's own output streamed
  under the batch name and everything appended to `elfie-requests/compile.log`.
- **A problem's stage is read from its node.** The compiler asked whether `Problem` should carry
  its origin; the loader only ever adds problems at `Use` nodes, so a problem at a `Use` is stage
  loader and any other is binder, and no field was added.
- **The CLI's own files live in `elfie-requests` under the root**, not the output directory, and a
  marker is only text that names a `.lfy` path; a marker naming a file outside the program is
  ignored. Both came from the first mechanical acceptance, which read the compile log, the request
  files, and test fixtures as claims about the program.

# Standard library (2026-09-20)

The package `elfie`, rooted at `lib/`, declares what was previously only inside the binder or in
prose: the context layer every entity has, the base data every value has, the system vocabulary
the compiler's own definitions speak, and the target vocabulary. `elfie.json` registers it as a
dependency for now; the workspace will load it implicitly once recompiled. Hash, git, and the
LSP/MCP protocols are deliberately not part of this round.

- **Members are the spelling for operations on a value.** An operation is a member whose value is
  an inline function: `$map: \`...\` = (transform: function) => Any[];`. A body that is a type
  expression is a signature (native for a `builtin` data, generated from criteria otherwise); a
  body that is an expression or block is written out in full and translated as written. Nested
  `fn`/`function` in a data body do not become members, so they are not used.
- **Criteria and tests for a member live in `with X$member { ... }`** directly after the member.
  The binder's definition already says a `With` whose current entity is the member attaches
  criteria to it; the compiled binder does not yet do it (verified 2026-09-20 on a scratch
  project: criteria=0 after such a block), so the next model compile must, and a test is added.
  Member tests put the receiver first in `input`.
- **`builtin` marks a whole declaration**, never a member: a trait on a member (`$x is t`) parses
  but binds nothing, and `t.apply(X$x)` is refused because a scope-layer member is a symbol, not
  an entity. A builtin data's instances are the target's native type and every signature member is
  a native operation; a builtin fn is bound to the target's standard library.
- **`Any`** (`d Any is builtin`) stands in where the language has no generics; callable parameters
  are typed `function` because an inline function type cannot appear in a parameter's type.
  Parameters still cannot carry descriptions; `///` lines of the form `name: text` describe them,
  as `def/mcp` already does. The grammar change is the first item of the next round.
- **Resolution rule the binder gains:** the context layer of an entity resolves in the members of
  the prelude data for its kind (`Entity`, then `Trait` for a trait, `Function` for a fn or
  function, `Enum`, `Member`, `Parameter`, `Module` likewise); the value layer resolves in the
  entity's own members first, then in the base data's members whose value is a function (so
  `rust.apply(global)` finds `Trait.apply`). A value of a primitive type resolves its members in
  `String`, `Number`, `Boolean`, `Object`, `List` (any list), `Template`, and `Function`. The
  `ContextProperty` enum goes away; `apply` stops being a special case in `bind`.
- **The prelude is the scope of `lib/main.lfy`** (its own symbols and its imports): every file
  scope's parent is that scope, except the files of the `elfie` package itself, which have none and
  use each other explicitly. System and target files are not in the prelude and are used as
  `use "elfie/system/path";`. The library is found through `lib` in `elfie.json`, else the
  `ELFIE_LIB` environment variable, else the copy shipped with the compiler.
- **The `elfie` package still gives no units.** Everything in it is builtin or written out in
  Elfie; the target's guidance says what each builtin binds to and translates written-out members
  where they are used. Compiling the library into a crate of its own is the round after this.
- **`d Entity` in `def/model/data.lfy` extends the prelude's `Entity`** and keeps only the fields
  the binder needs internally; likewise `FnEntity`/`TraitEntity`. Two `Entity`s would otherwise
  shadow each other in every model file.
- **The compiler's definitions name the library where they name an operation.** `def/workspace`,
  `def/generation`, and `def/cli` use `elfie/system/*` and `elfie/target/*` explicitly and write
  `[[Files.read]]`, `[[Path.relative]]`, `[[Json.stringify]]`, `[[Time.Instant.rfc3339]]`,
  `[[Process.stream]]` instead of prose; SHA-256 stays prose because hashing is not in the
  library. `elfie/system/file` is aliased (`Files`, `FileSystem`) because `File` is the
  workspace's data; `json` and `time` are aliased because both declare `parse`. A member of an
  aliased module's data is referenced nested, `[[Time.Instant.rfc3339]]`, and the binder checks it.
- **Guidance is ordered nearest first.** `Request.guidance` is the marker's criteria, then those
  of each extended trait transitively (rust, targetLanguage, target), each once, then those of
  traits applied with arguments (`cratePerPrefix`, `modulePerFile`) with `{{...}}` evaluated from
  those arguments. The compiled binder currently drops extended traits' criteria from
  `criteriaOf` (verified with `elfie_entity rust`), so the next compile must close that gap.
- **The binder learns a file's origin from `Source.origin`** (`program`, `library`, `prelude`):
  the loader sets it; the prelude scope is the `prelude` file's scope, `program` files have it as
  parent, `library` and `prelude` files have none. The model stays independent of `File.package`.
- **The `elfie` package root is the first source given:** a dependency entry named `elfie`, then
  `lib` in elfie.json, then `ELFIE_LIB`, then the copy the compiler was built with. A
  `Target.marker` must be `target` or extend it transitively, or the target is left out with a
  `LoadProblem`.
- **Context names of another kind yield undefined, not a Problem:** `@parameters` on a data
  resolves to `Function.parameters` and is undefined there, so `@parameters ?? @type` in
  `Test.input` stays meaningful; a name in no kind data at all is still a Problem. `add` on
  `@acceptanceCriteria` is the binder's own call, not a `List` member.
- **Primitive values resolve every member of their base data** (`s.length`), while an entity's
  value layer adds only the function members of its kind data (`t.apply`, never `t.identifier`).
- **The model keeps `Criterion` (contributor as `Entity`) and `Test` as extensions of the
  prelude's**, and overrides `Entity.acceptanceCriteria/traits/references` and
  `FnEntity.parameters` where its records are richer; `ListEntity` and `ContextProperty` are gone.
  `Hover.traits` carries an entity's trait identifiers so a native thing shows `builtin`.
- **Tooling:** `scripts/refcheck.py` resolves package paths through `elfie.json` and scans every
  package; a reference must start with a name, so `[[1, 2], x]` in a test is not one; a file that
  declares nothing (the prelude) is exempt from unused-use checks. `scripts/compiler-prompt.md`
  tells the compiler the library is a specification, never a unit.
