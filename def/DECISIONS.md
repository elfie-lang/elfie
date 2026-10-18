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

**The server writes marker lines and nothing else (2026-10-10).** During compilation the agent
writes code itself and calls `elfie_check`; the CLI records source maps and review records. The one
thing the server writes is a marker line: `elfie_mark`, offered only to a compiler session, inserts
one or rewrites the ids of existing ones. It writes only in that session's own batch's regions of
its target's output files, under its run's directory. A rename touches only markers naming the
file of the new id's unit; it writes what `Generation.rename` gives, which `compile` also uses
to move the markers of reworded criteria. An insertion is refused when the line falls in a region whose marker
names a file outside the batch. It does so because a 64-hex id is copied wrong often enough to matter, and the
server can copy one it has checked. Marker lines still pass through acceptance like any other line,
so the CLI stays the only path to "what was accepted". This replaces the earlier rule that the
server never writes a file. `elfie_report` and `elfie_submit` keep what they record in memory
until the CLI closes the session. A command tool runs the target's own command, which may write
what that command always writes, such as build artifacts, and never an output.

**Two entry points.** `serve` is the stdio server for editors and interactive agents. `host` is
served over HTTP on 127.0.0.1 for the length of one compile or verify, with one hosted session per
compiler or verifier run. Each hosted session is bound to its run's directory, plan, program, and
review.

**Tools:** `problems`, `find`, `entity`, `references`, `outline`, `grammar`, `units`, `request`,
`task`, `dependency`, `check`, `mark`, `submit`, `output`, `search`, `source`, `changes`,
`review`, `globalReview`, and `report`, plus the command tools `build`, `test`, `lint`, and
`format`. The compiler tools `task`, `dependency`, `mark`, and `submit` carry the trait
`compilerTool`: they act on the request a compiler session's `Binding` holds.
- `serve` offers every tool except `report`, the command tools, and the compiler tools.
- A compiler session offers every tool except `report`, and a verifier session every tool except
  `check` and the compiler tools, with the command tools limited as below.
- A hosted session offers a command tool only when a target of the session has a command with a
  line for its operation.

No resources, no prompts: one surface is enough to keep exact.

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
  EBNF document, produced by running `terminalDocument()`, now `Grammar.ebnf(terminal)`), and `for (const rule in
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
  (`tree.tokens.get`); it is strict for modules, data, enums, and types.
- **Function bodies** (`function`) are not executed at bind time except for their context statements
  (`@acceptanceCriteria`, `@test`, `where`, `with`, `for`); they are executed as code only when
  called from a template or trait argument (`Grammar.grammarDocument()`, now `Grammar.ebnf(rule)`).
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
  produces what it depends on. One request per batch sends guidance and interfaces once
  (superseded 2026-10-17, below).
- **Outcomes.** The compiler ends its report with `ELFIE: DONE`, `ELFIE: BLOCKED: <why>`, or
  `ELFIE: CLARIFY: <question>`; `outcomeOf` reads that line and the verdicts. Rejected batches are
  retried once with the problems; blocked and clarification stop the compile and leave a file
  under `elfie-requests/` for the person; failures retry once. `--continue` lets independent
  batches proceed past a stopped one (its meaning superseded 2026-10-17, below).
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
  `use "elfie/system/path";`. The library is found through the `elfie` dependency entry, else the
  copy shipped with the compiler (superseded 2026-10-17, below).
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

# Generics, mapped output access, independent verification (2026-09-20)

- **Criteria are never named.** A review, a rejection, or a problem says `<file>:<line>` and
  quotes the criterion. Names would not be unique, would be one more thing to keep in sync, and a
  criterion that needs a name to be understood is the thing to fix. Consequence: no coverage
  tool per criterion; coverage stays per entity, and a verifier's reviews are keyed by file, line,
  and entity.
- **Type parameters.** `TypeParameter`, `TypeParameters`, `TypeArguments`, `FunctionType` in type
  position, and a `Generic` postfix in expression position with a lookahead (the `>` must be
  followed by `;`, `,`, `)`, `]`, `}`, `=`, `(`, `.`, `[`, `|`, `&`, or `=>`), so that a member's
  type `= List<T>;` parses while `a < b > c` stays relational. `T[]` is sugar for `List<T>`,
  `Entity.itemType` is the first argument of `List`, display prefers `T[]`. The binder substitutes
  positionally and infers nothing else; an argument-count mismatch is a Problem. `Any` and the
  `function` stand-in leave the library in the round after the grammar compiles, since the current
  binary cannot parse angle brackets and the library must bind throughout.
- **Regions are markers with an end.** `Marker.end` is the line before the next marker in the same
  output, or the last line; derived at acceptance, never written by the compiler. A separate
  region type was rejected as a second copy of the same fact.
- **Entity-level changes are computed by rebinding the previous text** through the workspace's
  `change`, then comparing entities by identifier: kinds added, removed, definition, type,
  signature, criteria, tests, body. Mechanical, and what the compiler and verifier read instead of
  whole files.
- **Verification is a second command.** `elfie.json` names a `verifier` beside `compiler`; it runs
  after structural acceptance with the review request on stdin, read-only tools, no access to the
  generator's report. One JSON line per criterion and test: file, line, entity, status
  (satisfied, violated, unverifiable), evidence, note; end line `ELFIE: REVIEWED`. A violated
  review rejects the batch with `failure at <file>:<line>: <note> (<evidence>)` and the existing
  retry-once rule applies; unverifiable never rejects; `--no-verify` and a missing `verifier` skip
  it. `elfie verify <stem>` runs it standalone.
- **Verdicts go through the agent server (2026-10-10).** A verifier hand-copying hundreds of
  64-hex ids into one final message lost verdicts three ways: the message was truncated, ids came
  out garbled, and only the last message reaches stdout. `compile` and `verify` now host the agent
  server over HTTP with one session per run. The verifier submits each verdict with
  `elfie_report`, which checks the verdict and records it as it arrives. JSON lines in the final
  message remain only as the fallback where `elfie_report` is not offered. The review request
  lists region locations, not code. Reviews are incremental: a verdict is carried while the hash
  of its entity's regions is unchanged, and `elfie verify --full` re-judges everything.
- **Many verdicts per call: `elfie_reportMany` (2026-10-11).** Every turn re-reads the whole
  context so far, so turns, not verdicts, set a review's cost. Measured on generation/main (326
  verdicts): one `elfie_report` per turn took 403 turns at a mean 413k tokens of context per turn
  (29.5 min); many `elfie_report` calls per turn took 43 turns (14.9 min). `elfie_reportMany`
  takes one JSON line per verdict, with the keys of a review line, and handles each as
  `elfie_report` would. A bad line is an error for that line alone. It is offered wherever
  `elfie_report` is, so to verifier sessions only. The protocol and `scripts/verifier-prompt.md`
  now say to judge an entity's criteria together and submit them in one call, never one per turn.
- **The compiler on the agent server (2026-10-10, ROADMAP phase 3).** The rule is to push the
  obligations, pull the context, and act through tools.
  - `Binding.request` holds the request a compiler run was given, as `Binding.review` does for a
    verifier.
  - `elfie_task` repeats the obligations from that request: per unit the stem, file, reason, and
    outputs, and per entity each criterion and test with its id, place, and text. Each one is
    marked `new`, `kept`, or `unknown`, and the removed ones come with their text. The word is
    `kept` rather than "carried", which already names a review verdict that is not asked again.
  - `Request.removed` holds `RemovedRequirement`s: id, place, and text. `request` takes them while
    the previous lowering exists, because a removed criterion's origin belongs to that lowering
    and nothing later can read it.
  - In a compiler session, `elfie_request` for the session's batch returns the instructions of
    `Binding.request` itself.
  - `elfie_dependency` gives one entity of a dependency, as `Request.interfaces` holds it, with its
    lines of `interfaceOf`. The tool cannot be named `elfie_interface`, because `interface` is a
    reserved word.
  - `Generation.request` takes `hosted`, which the CLI sets for the compiler runs it hosts. A
    hosted request's interfaces part names each dependency's file, outputs, and entities, and points
    to `elfie_dependency`. Every other request still quotes the interfaces whole, so a compiler
    without the server is not left short.
  - `elfie_output` takes `text`: with `text` false it gives an outline of the regions without their
    code. An output file named by its path is now read with the markers it holds when the call
    arrives, rather than the recorded map, so a compiler that has just edited it sees its regions
    where they are. For the same reason, in a compiler session the maps of the batch's own units
    are read from the markers their output files hold at call time. Every tool that reads maps
    (`elfie_output` by entity or id, `elfie_source`) sees them there.
  - `elfie_search` greps the outputs of the session's targets with a Rust `regex` pattern. It gives
    each line with its region's marker and stops at 200 lines, saying how many it left out.
  - `elfie_mark` writes marker lines; see MCP above.
  - `elfie_submit` records done, blocked, or clarify in the session, and the last call wins.
    `Host.close` now returns a `SessionRecord` of the reviews and the submission.
    `Generation.outcomeOf` takes the submission and prefers it to the report's `ELFIE:` line, which
    is kept as the fallback. A `done` with no verdict is failed. A `done` message is not part of
    `Outcome.message`.
  - Raw Read and Grep stay allowed (ROADMAP 11) until the share of tool calls that go to the server
    is measured.
- **MCP gains `output`, `source`, `changes`, `review`**, all read-only: generated regions by
  entity, the definition line behind a generated line, the entity-level diff since acceptance, and
  the verifier's own request. Resources stay out; four tools keep the surface exact.
- **Writers' decisions folded in:** `ChangeKind.type` is spelled `declaredType` because `type` is
  a keyword and cannot name an enum member; `TypeGroup` is its own rule because `FunctionType`
  and a parenthesized type share a first token, and `triedBefore.apply(TypeValue, TypeGroup)`
  orders them; `Generic` binds at the access level and is tried before `RelationalOperation` at
  the same token; a type parameter's kind data is `Kinds.Parameter` (its default in
  `defaultValue`), so `kinds.lfy` did not grow; a generic reference is "the declaration seen with
  arguments", an entity whose type is the declaration; `Hover.owner` names the declaration a type
  parameter or member belongs to; type spelling is centralized in `Query.typeTextOf`; a criterion's
  place is its contributor's file and the line its `add` call or `where` begins; the verification
  retry is its own once-more, separate from the structural retry; a twice-failed verifier run
  still rejects on any violated review it did parse.
- **The library after generics.** `List<T>`, `Object<V>`, `Pair<K, V>`; `Any` is gone and no
  parameter is typed `function` any more. An inline function cannot declare its own type
  parameters, so a member whose result needs a fresh one (`map`, `reduce`, `flat`, `entries`,
  `apply`) is a top-level generic `fn` in the same file with the member referring to it
  (`$map = map;`), which puts those names in the prelude scope; a project declaration of the same
  name shadows them without a problem. `object` and `trait` are keywords, so such parameters are
  named `source` and `subject`. `Parameter.defaultValue` is an `Entity`.
- **What the first verified compiles found (2026-09-20/21).** The verifier's first run over
  the compiled repository gave 108 satisfied, 4 violated, 1 unverifiable: two contradictions in
  `def/workspace/main.lfy` (an absent elfie.json was both a problem and not; a package file was
  both in the program and out of `change`'s reach), one binder gap the library rewrite exposed
  (`Trait.apply` became a reference to a fn, and only inline-function members counted as
  function members), and one leniency the definition never allowed (an unresolved member on a
  trait added no problem). All four were fixed in the definitions, not the code, and the next
  compiles converged: model/main 61/0, workspace/main 47/0, generation/main 89/0, cli/main 63/0
  (satisfied/violated). Two process rules came out of it: source maps are recorded only after
  verification passes, so a batch the review rejects stays planned; and a batch compiled before a
  later batch changes the binder or loader may need re-requesting, since its tests ran against
  the earlier behavior (generation's stub-library fixture, twice now).

# Language gaps closed for v0.1

- **A trait is applied to a file with `t.apply(@)` at the file's top level.** A bare `@` resolves
  to the scope's current entity, which in a file scope is the file's anonymous entity; generation
  already builds every entity of a file whose anonymous entity carries the marker. Alternatives: a
  file header statement (rejected: a new statement kind every stage must learn) or `global` only
  (rejected: no per-file choice with several targets). Closes issue 2 above.
- **A keyword where a name belongs is reported as such.** `ErrorNode.keyword` is set when an
  identifier of the same text would have let the open rule continue, and the diagnostic reads
  `d is a keyword and cannot be a name`. The keyword stays reserved everywhere; making `d`
  contextual or renaming it was rejected. Closes the `x = c ?? d;` note above.
- **Parameters stay without descriptions.** No grammar change, and `///` lines stay a
  convention the binder does not read. Issue 1 above is left as is.
- **`Entity.sideEffects` is gone**; `sideEffects` is a field of each criterion, set by its `add`
  call (`lib/criteria/criterion.lfy`). Closes issue 3 above.

# Compile data in elfie-compile (2026-09-30)

- **Source maps move from one file per target to one file per unit.** They were a single
  `source-map.json` in each target's output directory, rewritten whole on every acceptance. They
  now live at `elfie-compile/maps/<target>/<stem>.json` under the root, one JSON list per unit,
  mirroring the definitions tree (`def/cli/main.lfy` maps to `elfie-compile/maps/rust/cli/main.json`).
  Per unit rather than per output because the unit is what is planned, accepted, and recorded:
  staleness reads one unit's maps, and acceptance writes one file. Alternative rejected: one file
  per output, mirroring the output path, which would split one unit's record across files and
  still need every file read to plan a unit.
- **What that buys.** Accepting a unit writes its map file and nothing else, so a compile of one
  unit changes one map file and a commit shows which units a compile touched. Maps are written
  with sorted keys and in output order, so equal maps give equal bytes; outputs re-accepted
  unchanged leave the file untouched rather than bumping `generated`. `--target` opens one
  target's folder only. Deleting a definition file removes its map file on the next compile.
- **`elfie-compile/` is the home of compile-time data and is checked in.** `elfie-compile/cache/`
  is the exception: `elfie init` adds `/elfie-compile/cache/` to `.gitignore`, and nothing may
  rely on it. Deleting it changes no plan or verdict, only time; the one request it can change
  is that of a batch that would have started from the work its last rejected run kept there (see
  "Mechanical steps and scheduling" below; superseded 2026-10-17, below, where the cache holds a
  kept change set). Anything that is the only record of something belongs outside `cache/`. This
  repository ignores it too.
- **Reading and recording are `Generation.sourceMapsOf` and `Generation.record`**, shared by the
  CLI (`check`, `compile`, `verify`) and every agent server tool that reads maps, replacing the
  reading rule each of them spelled out on its own.
- **Migration.** A target with no maps folder still reads `source-map.json` from its output
  directory; the next compile that is not a dry run records each unit's maps in its own file and
  removes `source-map.json`. `crates/source-map.json` stays until the compiled CLI does this.
- **Issue:** `elfie-requests/` (logs, blocked and question files, reviews) still sits under the
  root beside `elfie-compile/`. Moving it under `elfie-compile/` was left out of this change.

## Targets in source (2026-10-04)

Targets moved out of `elfie.json` and into the program. Four designs were compared: records written
where they are used, traits as targets, a `target` declaration kind, and named targets. The last
was chosen.

- **A target is an `ace const` of the project holding the package elfie's `Target`.** The const's
  name is the target's identifier, so `--target`, `elfie-compile/maps/<identifier>`, and the MCP
  tools keep working. An ace const has no lowered node, so a target is never generated and the file
  declaring it gives no unit.
- **`elfie.json` is read permissively.** A key the loader does not read, in the manifest or in one
  of its entries, is ignored with no problem, so the retired `targets`, `output`, and `native`
  keys, and keys meant for other tools, never stop a load. A key it does read with a value of the
  wrong kind is still a load problem.
- **Roles.** A `Target` has a slot per role: language (1), runtime (0–1), platforms, ecosystem
  (0–1), frameworks, interfaces, layout (1), and extra layers. The role traits live in
  `lib/target/`; every layer comes from a package. Layers state capabilities as strings
  (`provides`, `requires`), and the loader checks them.
- **Layers are applied to the const** (`Target`'s rule in `lib/target/main.lfy`), in guidance
  order. This reuses `apply`: each layer's criteria are rendered from its arguments, and its
  knowledge, commands, and values land on the const. Values are kept in setter order, so the
  list-valued `provides`, `requires`, and `outputExtensions` gather every layer's items.
- **Selection is `@targets`**: an entity's own list, else its file's, else `global`'s. Reaching an
  entity not built for the same target is a plan problem.
- **Knowledge and commands** are context lists on every entity, filled like criteria. The request
  lists the commands. Acceptance never runs one. The agent server's command tools (`elfie_build`,
  `elfie_test`, `elfie_lint`, `elfie_format`) run the target's declared command for their
  operation only when an agent in a hosted session calls them, in that run's directory. A session
  is offered such a tool only when its target has a command with a line for that operation. There
  is deliberately no flag saying whether the compiler knows a layer: what it may not know is
  knowledge, and how to use it is criteria.
- **The compiler's record keeps the name `Target`** (`def/workspace/data.lfy`). Files that need the
  library's record take it as `Targets.Target`, so nothing else had to be renamed.
- **Migration.** The step-1 compile is run by the binary that reads the old `elfie.json`, so the
  new binary carries no fallback for it. The switch to `def/targets.lfy` and `vendor/` happens
  right after the new binary is built.
- **Issue:** the formatter wraps a long `fn` signature inside a generic output type
  (`=> Layer<` / `A,` / `> {`); `layer`'s description was shortened to stay on one line.

# Mechanical steps and scheduling (2026-10-10, ROADMAP phases 4 and 5)

## Reworded criteria (phase 4, item 12)

- **One pairing, stated once.** `changes` calls a removed and an added criterion (or test) of one
  entity *reworded* when, counting from the first, the one at some position then has an id that is
  none of the entity's now, and the one at the same position now has an id that was none of its
  then. `Generation.reworded` turns those pairs into `Rewording`s (`id`, `replaces`, and the old
  `text`), keeping only those whose new id is a local criterion or test of the unit. A criterion
  that moved as well as changed is not paired; the compiler still sees it as removed and new, and
  `elfie_mark`'s rename is for that case.
- **One rewrite, shared.** `Generation.rename` rewrites a unit's markers from old ids to new ones.
  It touches only markers naming the unit's file (or a file whose entities moved into it), and
  keeps every other character. `elfie_mark`'s rename now writes what it returns, so the two never
  differ.
- **Before every request.** Whenever a compiler is named and the source a unit was last
  accepted from is recoverable through git, `compile` rewrites the unit's markers in the batch's
  directory before each call of `Generation.request`. The rewrite is idempotent, so retries and
  new copies just repeat it. `compile --accept` does the same in the root before it checks a
  unit. Requests written for a compiler run by hand are never preceded by a rewrite. In place,
  the rewritten ids sit in the root until the batch is accepted, which is intended: they name
  criteria the unit has.
- **The request.** `Request.reworded` holds the pairs per file. `Request.removed` leaves out the
  ids a rewording replaces. In a unit's section a reworded criterion is marked `reworded`, followed
  by a `replaces <old id> — <old text>` line. The section says that compile has already moved its
  markers, and that the code is read against the new text. `elfie_task` gives the same mark and
  line, and counts `reworded` beside new, kept, unknown, and removed.
- **Reworded-only batches skip the compiler.** `Request.rewordedOnly` lists the units of the
  batch for which all of these hold: the reason is `requirements`, the previous source is known,
  nothing is removed, every id is answered or reworded, and every dependency's interface hash is
  the recorded one. When every unit of a batch is listed, and a compiler and a verifier will run,
  the batch's first run has no compiler. Its rewritten outputs go straight to `accept` and then to
  review. This requires two more things. First, every output must still be the text that was
  accepted: `SourceMap.text`, a new SHA-256 of the output as accepted, is compared before the
  markers are rewritten. Second, the batch must have taken no kept work. (Batches and kept work
  superseded 2026-10-17, below: the path is taken per unit, and never by a unit whose request
  lists a foreign edit.)
  - A rejection by `accept` sends the batch to the compiler as its first run, and the rejection
    counts toward no retry.
  - A violated review makes the batch's run once more the compiler's first run, with the problems.
  - Without a verifier the compiler runs as before, because nothing else would read the new text
    against the code.
- **Retry counts count real runs.** The first and second rejected outcome, failed run, and
  verifier report count only runs of the compiler and the verifier. A first run that skipped the
  compiler (reworded-only or dependency-only) is not one.
- **A reworded criterion is always reviewed.** Its old id's record is dropped by `reviewsOf`
  (the id is no longer the unit's), and `review` carries a record only for the id it holds. This
  holds even when the entity's regions are unchanged, because a rewording may change what is
  asked.
- **Marker comments leave the region hash.** `ReviewRequest.hashes` now hashes each region line
  with the marker comment it carries removed. That is everything from the target's marker comment
  opener before `@lfy` to the end of the line, plus the white space before it. A line left empty
  is dropped. Code on a line that also carries a trailing marker still counts. Without this,
  moving a reworded criterion's marker, or adding one, would send every other criterion of the
  entity to review again. **Cost:** every recorded verdict was hashed with its marker lines, so
  the first compile or verify after this lands re-judges every criterion once.

## Dependency-only batches (phase 4, item 13)

- When every unit of a batch has the reason `dependency`, and the target has a command with a
  line for both build and test, `compile` runs the build and then the test before any compiler.
  This also requires that every output of its units still hashes to its `SourceMap.text`, and
  that the batch took no kept work. Otherwise unreviewed code, such as a rejected batch's work
  left in the root, or a shared output another batch changed, could be recorded without review.
  - They run as `elfie_build` and `elfie_test` run them with no filter and no scope, in the
    batch's directory, with the same 30-minute stop.
  - When both succeed, each unit's outputs go to `accept` as they are. The source maps are
    recorded with the dependency signatures `accept` computes now. No verifier runs, and no
    review record changes: no code changed, so the region hashes and the verdicts still hold.
    The batch counts as "accepted on its build and test", which the verifier criteria exclude.
  - When either command fails, or `accept` rejects a unit, the compiler runs as the batch's first
    run, and the failure counts toward no retry. A target with no build or test line goes
    straight to the compiler.
- **Mixed batches go to the compiler.** Both mechanical paths apply only when every unit of the
  batch qualifies. A dependency-only unit in a batch with its changed dependency cannot be
  checked by building before that dependency is regenerated (superseded 2026-10-17, below).
- **Issue:** `plan` puts a dependency-only unit into the open batch that holds its planned
  dependencies, so the commonest dependency-only unit (one in the same directory as what changed)
  never forms a batch of its own, and so never takes this path. Planning dependency-only units
  into batches of their own would let it apply. Now that dependents start on structural
  acceptance (below), the cost is one more request when the build fails. This was left for a
  decision. (Closed 2026-10-17, below: with no batches, every dependency-only unit is judged on
  its own.)

## Review off the critical path (phase 5, item 14; superseded 2026-10-17, below)

- **In copies only.** With `--jobs` above 1 in a git repository, a batch may start once every
  batch it depends on, directly or transitively, either has its files merged into the root, or
  has every unit accepted and is not about to run the compiler again (it is being reviewed). In
  place there is one tree, so batches still run one at a time and wait for their dependencies to
  be done.
- **Done is not merged.** A done batch may still wait for its own dependencies to merge. Every
  scheduling rule therefore asks whether a batch's files are merged into the root, not whether
  it is done.
- **What a dependent sees.** Its copy is made from the root. Then the *accepted changes* of every
  batch it depends on, transitively, that is not yet merged are merged in, in plan order, the
  same way a batch is merged into the root. A batch's accepted changes are a criterion now:
  each file of its copy that differs from its base, was added, or was removed, with what the copy
  held for it the last time every unit was accepted. The base is taken after that merge, so the
  batch's own changes never include its dependencies'. When the merge conflicts, the batch waits
  until that dependency is merged.
- **Merge order.** A batch is merged into the root only once every batch whose accepted changes
  it took is merged, and the files that merge wrote hold what each change the batch took held. So
  the root still holds only reviewed work. Global review runs in the root after every merge and
  after any batch that is to run again has done so.
- **The interface cannot change on a retry.** A retry runs from the same plan, so
  `interfaceOf`, and the `SourceMap.signature` that hashes it, is the same as on the first
  accepted run; comparing signatures would never re-plan anything. What a retry can change is
  the code a dependent was built against. So the check is mechanical, at merge time:
  - When the dependency's merge wrote what each change the dependent took held, the
    dependent's results stand.
  - Otherwise the files the dependency's merge wrote are merged into the dependent's copy (and its base),
    and the target's build and test run there. When both pass, the dependent is merged.
  - When either fails, the merge conflicts, or the target cannot build or test, the dependent
    runs again from a new copy. That copy takes its own earlier changes as kept work, and its
    compiler and verifier runs are counted afresh.
  - A batch that was done and runs again is no longer done. The batches that took its accepted
    changes are checked the same way, against what its merge writes after it runs again.
- **A dependency that stops** stops every dependent that took its changes, as `--continue`
  already stops dependents, and none of them is merged. A run still going in such a dependent is
  stopped with its process group, and what it gives is ignored.
- **When the compile stops** (without `--continue`), every run still going is stopped the same
  way, and nothing of those batches is merged or recorded. This replaces "the batches already
  running finish as they would", since nothing they produced could be merged.
- **Maps are recorded only by the merge in copies.** Every "recorded … done" criterion now says
  that recording happens at once in place, and only when the files are merged in a copy.
- `--jobs` now counts batches running the compiler, the verifier, or a target command, not
  batches in flight, so a batch that waits to merge holds no slot.

## Keeping rejected work (phase 5, item 15; superseded 2026-10-17, below)

- When a batch run in a copy stops rejected (a second rejected outcome, or a second review with
  a violation), `compile` writes its kept work to `elfie-compile/cache/kept/<batch>.json`. The
  file holds:
  - the target;
  - per unit, in batch order, its stem, the SHA-256 of its `Unit.text`, and its requirements hash;
  - the problems that stopped the batch;
  - per changed file, its path, its base text, and its text (null for added or removed).

  The copy is still left in place for a person.
- **Taking it.** The next compile takes that work into the batch's new copy, after its base is
  kept, only when all of these still match: the batch identifier, the target, the unit stems, and
  each unit's text hash and requirements hash. Stale work, written against other code or other
  criteria, is never taken.
  - Each file is merged as the three-way merge `git merge-file` gives.
  - A file that conflicts is left as the copy holds it. That is neither a rejection nor a reason
    to run again, and the root-merge retry rules do not apply.
  - A `resuming` progress line counts the files taken and the files left.
  - Every request for the batch then says that its existing outputs hold work a run rejected and
    that was never accepted, and lists the problems that rejected it.
- **Removal.** The kept file is removed whenever the batch's source maps are recorded, in any
  mode. Before any batch runs, a kept file naming a stem that is no longer a unit of the plan is
  removed, as orphan maps and review records are.
- The file lives under `cache/` (not `batches/`, where a copy for a batch named `x` would
  hold the file of a batch named `x/y`), because losing it costs only time. That is why the cache
  criterion now allows exactly this one difference in a request.
- In place (`--jobs 1`, or no git), nothing is kept or taken: a rejected batch's outputs already
  stay on disk in the root, and the next compile starts from them. Their text no longer matches
  `SourceMap.text`, so neither mechanical path keeps them as they are.
- Blocked, failed, and questioned batches keep nothing; neither does a batch stopped because its
  dependency stopped.

## Progress

- Three steps were added: `resuming` (a copy takes kept work), `rewording` (a unit's markers were
  rewritten, with counts), and `building` (build and test in place of the compiler, or before a
  merge). The first two are drawn like `requesting` and `building` like `compiling`. (The
  meanings of `resuming` and `building` superseded 2026-10-17, below.)

# Knowledge, experiments, and output layout (2026-10-10, ROADMAP phase 6 and knowledge)

## Repository knowledge

- Facts compiler agents kept rediscovering (the CLI's package is `elfie`, not `elfie-cli`; no
  `cargo fmt`; shared outputs; tests that load `def/` must not pin counts) are written once in
  `def/knowledge/repository.md` and added to the rust target in `def/targets.lfy` as knowledge of
  kind reference, quoted. The source is relative to the repository root, because the file
  declaring it is the project's own.
- It is quoted, not named, because the compiler may not read `def/` directly. The verifier is
  given no knowledge, so its prompt names the package names for `scope` itself.
- Knowledge re-plans no unit: it is no part of a unit's text, criteria, or interfaces. It reaches
  the next request of every unit.

## Experiments (phase 6, item 17)

- **Retries that answer problems resume.** Every run of the compiler or the verifier gets
  `ELFIE_SESSION`, a fresh UUID. Three runs also get `ELFIE_RESUME`, the agent session the run
  before them used: the compiler's run once more after a rejected outcome, the compiler's run once
  more after a review with a violation (when the compiler ran before), and the verifier's run once
  more after that. The run before must have been in the same directory, since an agent session
  lives under the directory it ran in: the copy the batch was given when it last started, or the
  root.
- No other run resumes: not a first run, not the compiler's first run after a skipped one
  (reworded-only, dependency-only), not a batch that runs again from a new copy, not the global
  review or a batch it planned, and not a retry after a failed run, an exit code of -1, or a
  review that ended without every verdict. Those runs may have left a session that is broken or
  too long to continue.
- A resumed session keeps its id, so every later retry resumes the same one.
- **The scripts may start afresh.** They pass `--resume` only when the session's file is in
  Claude Code's project store (`$CLAUDE_CONFIG_DIR` or `~/.claude`, then `projects/`) and is under
  20 MB; otherwise they pass `--session-id "$ELFIE_SESSION"`. They log the flag they pass to
  standard error. The CLI's criteria are only about the variables.
- Nothing about how many times a batch runs changes, and the retry's request is still given whole
  on standard input.
- **Model tiering** is an operator setting, not a definition: `ELFIE_COMPILER_MODEL` and
  `ELFIE_VERIFIER_MODEL` are read by the scripts and passed as `--model` when set.

## Smaller outputs (phase 6, item 16)

- The rust target's guidance says that when an output file of a unit would exceed 2,000 lines
  (code and tests together), the unit's code is split into submodule files of at most 2,000
  lines. Each holds one or more entities, or one concern of an entity too large for one file
  (cli/main's `main`). They are declared with plain `mod` lines, never `#[path]`, and each file
  begins with a marker. Tests are split the same way.
- The module file keeps, of the unit's own code, only `mod` lines, the `pub use` lines other units
  need, shared helpers, and the entry point of `main.rs`. Lines other units or the layout put
  there stay, because `lib.rs`, `main.rs`, and `mod.rs` files are shared. Names other units use
  are re-exported, so no dependent changes.
- A submodule's file is no other unit's output, and no path a future unit's stem would spell.
- The request's outputs part used to say that names and structure are kept. It now allows this
  split as the one change of structure. `cargoCrate`'s stop rule accepts a directory whose module
  file sits beside it (`data.rs` for `data/`).
- Guidance re-plans nothing (`compile --dry-run` plans the same units before and after). Each
  large module is split the next time its unit is compiled, one unit at a time, or by
  `compile <stem>`.
- **Moving a co-owner's markers is not taking them away.** A split carries markers that name
  another unit's file (they sit in the unit's own code) into the new submodule files. `accept`
  used to look for a co-owner's recorded markers only in the file they were recorded for, so the
  first real split (generation/main's `mod.rs` into 21 files) was rejected over generation/data's
  regions. It now counts them across every output of the unit and every other file of the output
  directory (`accept`'s `elsewhere`). For each (file, entity, requirement), there must be at least
  as many as across the co-owner's recorded maps. Counting rather than finding stops a unit from
  dropping one of two regions with the same name. A marker that is the unit's own (it names the
  unit's file, or an entity moved into the unit together with that entity's file) stays the unit's
  to write or drop.
- **`SourceMap.text` hashes the unit's own regions, not the whole output.** The text is stripped
  as a review's region hash strips it. A write to `lib.rs` by another unit used to make every
  co-owner's text stale, which sent its reworded-only or dependency-only batch to the compiler.
  Hashing the whole file again after each write would have hidden an unreviewed edit made inside
  the co-owner's regions. **Cost:** maps recorded before this hash the whole file, so each unit's
  next reworded-only or dependency-only batch goes to the compiler once.
- **Co-owners' maps are re-derived once a batch is recorded.** Otherwise a co-owner's map would
  keep naming regions that moved, and its next accept would be judged against that stale map.
  `Generation.rederive` reads markers from the files as they are. It carries over every other
  member output by output, `text` included. An output it had no map for copies the shared
  members only when all of its maps agree, and gets an empty text. If they disagree, its maps are
  left as they are. Its reason to be planned never changes, no compiler or verifier runs for it,
  and its review records are untouched. Regions that grew (by line count) print a warning. Nothing
  rejects them, because `accept` has no earlier text to compare with. The carried text no longer
  matches a grown region, so such a region is never kept on that hash. A compiler session's maps
  for its own units come from the same `rederive`, so regions are bounded the same way everywhere.
- **The last accepted source must match the requirements too, with a fallback.** Lowered text
  leaves out criteria, so a committed revision that only reworded a criterion lowered to
  `SourceMap.hash` and was taken for the accepted source. `reworded` then compared that revision
  with itself, and the reworded-only path never fired. The revision must now also match
  `SourceMap.requirements` (stated once on `SourceMap`). Old revisions are lowered against today's
  workspace, so a unit whose criteria come from a trait or a `with` block in another file may
  never match both. In that case the latest revision that matches the hash alone is `previous`,
  and `request` is told so (`hashMatched`). Nothing is then paired as reworded. `removed` lists
  the ids in `answered` that the unit no longer has, with no text. The reworded-only path needs
  the revision that matches both. `elfie_changes` says when it fell back.

# Configuration, file names, and list item types (2026-10-17, from ~/elfie)

- **`elfie.json` is a `Configuration` record** on `Workspace.configuration` and
  `Package.configuration`: `name`, `version`, `source`, `dependencies`, `exports`. Keys it does not
  declare (`compiler`, `verifier`, old `targets`) are still ignored without a problem. A declared
  key of the wrong type is a `LoadProblem` and reads as undefined; the rest still loads.
- **A dependency is one string:** a path (`./`, `../`, or absolute), a git URL (`git@`, `git+`,
  `://`, or `.git` at the end), or otherwise a version. Only paths load; a version or git URL is a
  `LoadProblem` until fetching is defined. A package's own dependencies load too, resolved against
  its root, and a `use` in a package's file sees only that package's entries.
- **A `use` names a package by the longest run of leading segments that is a dependency key**, so
  scoped names such as `@elfie/targets` work. A package's `exports` map a path after its name to a
  file or directory under its root, and win over a file at the path the key spells. Exports never
  name entities: the loader resolves files before anything is bound.
- **The `elfie` package is a dependency entry or the built-in copy.** The `lib` key and
  `ELFIE_LIB` are gone. A package's own `elfie` entry is ignored. **Migration:** `elfie.json` must
  change `{ "root": "lib" }` to `"./lib"` once `workspace/main` is recompiled; the binary before
  that reads only the object form.
- **`source` may list several directories**, so `Workspace.sourceDirectories` replaces
  `sourceDirectory`. `File.name` is a file's path in its source directory, without `.lfy`; where
  two directories hold the same path, each such file takes the shortest run of trailing segments
  of its path that no other has. A package file's name is the package's name and its path in the
  package, the same text a `use` spells. `Unit.stem` and the interpreter's `use` alias read it.
- **`typeTextOf` has its own file, `def/model/type.lfy`,** so changing how types are spelled does
  not change the data every unit uses.
- **`itemType` belongs to list types.** `Kinds.Array` is the kind data of `List` seen with one type
  argument, and `ListEntity` says its item type is that argument. A list-typed const reads it
  through its type; a bare `List` has none. `FnEntity` and `Entity` no longer restate the
  prelude's `parameters`, `identifier`, and `definition`.

# Unit compiles that always build (2026-10-17, ROADMAP-units phases 1 to 5)

The compile of 2026-10-17 showed what batches cost. `workspace/data`'s new required fields broke
struct literals in units compiled later, so it could not pass a gate that asked the whole workspace
to build after every batch. `interpret/main` was rejected twice for code in `workspace/load.rs` that
only a later batch could regenerate. `workspace/main` was reviewed clean and then discarded,
because units it depended on were rejected. And a unit's request depended on which other units
happened to be stale with it. Batches, per-batch copies, and kept work per batch are gone: a
compile runs one unit at a time, bottom up, the code builds after every unit, and the root moves
only when the whole compile is done.

## Guarantees

- **G1 Order.** Units run one at a time, in `Plan.units` order, which comes from the plan alone.
- **G2 Requests.** A unit's request is built from its definitions, the interfaces of its
  dependencies, the target, and the current text of its outputs, with every edit another unit made
  to those outputs listed. It never depends on which other units are planned. This holds for the
  definitions, not the code: a touched unit's request carries code an earlier run wrote, as
  `Request.existing` always did, now named by the unit that wrote it.
- **G3 Always builds.** After every accepted unit, the target's build passes for the whole
  workspace.
- **G4 Always tests.** After every accepted unit, every test passes or fails only by reaching a
  stub.
- **G5 Bounded reach.** A unit edits another unit's outputs only inside that unit's regions, only
  for a unit later in plan order, and only as a stub or as a capped fix.
- **G6 Finishes.** Every stub sits in a later unit's code, and a unit holding a stub in its own
  regions is never accepted, so a bottom-up compile ends.
- **G7 Root.** The root takes every unit of the compile at once, and only when no stub is left, the
  whole build, test, lint, and format check pass, every planned unit is accepted and reviewed, and
  the global review passes.

## Stubs and fixes

- **A target says how to stub.** `targetLanguage` gains `stubLine`, the statement that replaces a
  body, whose `{{unit}}` (the stem of the unit writing it) and `{{reason}}` are filled in when it
  is written, and `stubMarker`, the text every stub line holds and every reached stub prints.
  Rust's is `todo!("elfie stub: {{unit}}: {{reason}}")` with the marker `elfie stub:`; Python and
  TypeScript set their own. The line is a quoted string, not a template, so its braces survive
  until a stub is written. Both may be undefined, for a language whose outputs have no body that
  runs, and then nothing is stubbed, only fixed.
- **Stubs and fixes are the only foreign edits.** A foreign edit is an edit inside a region of
  another unit's output (`ForeignEdit`: `by`, `owner`, `path`, `kind`, the region's marker, its
  lines before and after, and `note`). `accept` classifies every changed line outside the unit's
  own regions:
  - in a shared output (`lib.rs`, `mod.rs` lines), as before;
  - in another unit's region, rejected unless that unit is in the run's reach and the region's
    markers are unchanged; then a **stub** when the diff only removes lines and adds stub lines
    naming this unit, and a **fix** otherwise;
  - in no unit's region and not a shared output, rejected, as before.
- **Fixes are capped; stubs are not.** `fixLines` is 10 changed lines per region and
  `fixUnitLines` is 40 per touched unit. Anything bigger must be a stub. They are two `ace const`s
  in `generation/data`, so they can be tuned from measurements, and `Reach` carries them so the
  request and the agent server state the same cap. A rewritten line counts once: the count is the
  larger of the lines a line diff removes and the lines it adds. Every unit a run fixes needs a
  note through `elfie_touch`, or the run is rejected; a stub's note is the reason its line gives.
- **The reach is every later unit of the target that has outputs**, not only the units that depend
  on this one, because a unit can break a later unit's code through a shared output or a test as
  well as through an interface. It is read from `Plan.units` alone, never from reasons.
- **The cap is measured against the files as they were before the unit's first run** since it was
  last accepted (`Binding.before`), so a retry does not start its count again.
- **A stub line left in the unit's own regions rejects it** (G6). A stub the final gate finds is a
  defect to report, not a unit to run again.

## The touched reason

- **`Reason.touched`:** another unit edited its outputs during this compile. `plan` is handed the
  foreign edits the change set records since each owner was last accepted, and plans the owner as
  `touched` unless it already has a reason. A touched unit is always later than the unit that
  touched it, since a reach holds only later units and the order never depends on what is planned.
- **The request lists the edits** (`Request.foreignEdits`), each with the unit that made it, its
  kind, path, marker, note, and lines before and after, so code another run wrote into the unit's
  outputs is never taken for its own. Each stub line is to be replaced by code that does what the
  unit's criteria and tests ask.
- **A foreign edit changes the owner's region hash**, so its earlier verdicts on that entity no
  longer count.
- **The touched-fix path needs no compiler.** When a unit's only reason is `touched` and all its
  foreign edits are fixes, its outputs go to `accept` as they are, the unit gate runs, and the
  verifier judges only the entities whose regions were fixed. When both pass it is accepted, as
  the dependency-only path accepts on build and test; otherwise the compiler runs as its first
  run, with the problems, and the failure counts toward no retry. A stub always goes to the
  compiler, because a stub is code that must be written again.
- The reworded-only and dependency-only paths stay, per unit, and are never taken by a unit whose
  request lists a foreign edit.

## Requests, the unit gate, and the agent server

- **`Request` is for one unit.** `unit` replaces `batch`, the maps keyed by path collapse to the one
  unit, and `foreignEdits` and `reach` are new. `ReviewRequest`, `Binding`, and `compilerTool`
  name one unit; `Outcome.verdicts` is a single `verdict`, and a `Verdict` lists the foreign edits
  it made. `Batch`, `batchUnits`, `batchCharacters`, and `Plan.batches` are gone: `Plan.units` is
  the schedule.
- **What is the same for every unit of a target comes first** in the instructions (role, guidance,
  native dependencies, the target's knowledge, commands, globals, rules, stubs, tools, report),
  then the interfaces, the unit, its foreign edits, its outputs, and its reach. A prompt cache can
  reuse that prefix, which recovers the shared context batches existed for.
- **The unit gate** follows `accept`: the whole workspace builds, its tests pass with
  `ALLOW_STUBS`, and lint passes on the unit's own lines. Lint counts only those because a stub
  leaves the replaced body's parameters unused, and clippy's `-D warnings` would otherwise fail
  every unit after the one that stubbed. Then the verifier judges the unit, with earlier verdicts
  carried. A failed gate is a rejected outcome, and retry counts stay per unit.
- **`ALLOW_STUBS`.** A test command gets `1` when its caller allows stubs and the empty string
  otherwise. With it set, a line succeeds when the outputs build and every failing test failed by
  reaching a stub; a line that ignores it stays strict. Cargo's check is written inline in the test
  line, in POSIX `sh` and `awk` over libtest's report, not as a script in `vendor/rust` as
  ROADMAP-units item 6 proposed, because a command runs in the target's output directory and
  cannot find a package's files. `elfie_test` gains `allowStubs`.
- **`def/targets.lfy`'s gate is two criteria:** after every unit the workspace builds and every
  test passes or fails only at a stub; at the end of a compile `cargo build`, `test`, and `clippy`
  pass without warnings. `targetLanguage`'s quality criteria split the same way.
- **The tools take a unit.** `elfie_units` lists units and reasons, with no batches;
  `elfie_request` and `elfie_review` take a unit; `elfie_task` loses its unit argument.
  `elfie_reach` gives the later units this run may edit, the regions it has edited (or every
  region of one unit it names), and what is left of each cap. `elfie_touch(unit, note)` records why the run edited a unit
  (`Session.touches`, handed back in `SessionRecord`). The scripts read `ELFIE_UNIT` in place of
  `ELFIE_BATCH` and `ELFIE_UNITS`.

## The change set

- **One copy per target per compile.** Every planned unit of a target is worked in the target's
  change set (`ChangeSet` in `cli/data`): its units in plan order, each with its state, reason,
  and hashes, and every foreign edit recorded, appended to and never reordered. The copy is
  `elfie-compile/cache/changes/<target>/copy`, its base sits beside it, and the record is
  `changes/<target>.json`. Maps and review records are written in the copy; nothing in the root
  changes but through the merge.
- **Kept and resumed.** A compile that stops keeps the change set and its copy. The next compile
  resumes it while every accepted or reviewed unit's `Unit.text` hash and requirements hash are
  unchanged and the planned units are the same, in the same order. The first unit whose
  definitions changed, or where the order first differs, runs again with every unit after it; the
  units before it keep their state. When no unit would keep its state, the change set is made new.
  Only outputs, maps, and review records are a change set's own files; every other file is taken
  from the root again, so a resumed change set reads the definitions as they are now, but nothing
  under a build directory (`target`, `.git`, `node_modules`) is removed, so a resume does not
  rebuild from scratch. Losing it costs only time, which is why it lives under `cache/`.
- **A kept unit stays planned.** A unit a kept change set accepted, with unchanged definitions, is
  planned at its place with the reason it had there, even when the root's maps would not plan it.
  Without this, a compile of units requested by name could never resume: naming them again runs
  them again, and not naming them leaves them out of the plan, which differs from the kept order.
  Found in the first compile with the new binary, which ran mcp/main twice for nothing.
- **A unit that stops without being accepted** has its outputs put back as they were before its
  first run in the compile, and what it wrote is saved under `elfie-requests/<name>.rejected`. The
  copy then still builds for the units after it, and a resumed change set never starts a unit from
  code no run accepted. A unit rejected after its retries stops the change set, and nothing merges.
- **`--continue` stays, with a new meaning:** units that depend on a stopped unit are skipped,
  units that do not still run in plan order to surface their problems, and none of them merges
  without the rest.
- **`--jobs` is removed** until units that share nothing can run in copies of their own
  (ROADMAP-units phase 6), when it comes back with that meaning. Running one unit at a time is
  what makes the order, and so every request, deterministic.
- `compile --dry-run` and `compile --accept` make, resume, and remove no change set, and work in
  the root.

## The final gate and the merge

- **The final gate** runs on each change set before the root takes anything: no output line is a
  stub line, read as accept reads one (a whole trimmed line that is the stub line with any unit and
  reason); the build, the test without `ALLOW_STUBS`, the lint, and the format check pass
  in the copy; and every planned unit is accepted and reviewed, none violated. With `--no-verify`
  or no verifier, every unit must be accepted, none is reviewed, and no global review runs.
- **Stub lines, not the marker.** The roadmap's gate looked for any line holding the stub marker.
  The first compile with the new binary failed there on the compiler's own tests and fixtures,
  which quote the marker inside strings; a stub line is a whole line, so a quoted one never matches.
- **The global review** then runs once, in a copy, because what it judges is what the root is about
  to take; with several targets, in the last target's copy with the others' changes written in.
  Units it plans run again in the same compile, then the final gates, then the review once more.
- **The merge is all at once.** The root takes every change set of the compile together: each file
  a change set changed, added (when git would track it), or removed. A root file unchanged since
  the base becomes the copy's; one changed since is merged three ways with `git merge-file`. No
  root file changes until every file merges without a conflict, and a conflict changes nothing and
  keeps every change set. Then each change set's record, copy, and base are removed.
- **Progress** loses the batch: every line names its unit, and `touched`, `stubbed`, `finalGate`,
  and `merging` are new steps. `resuming` now means a kept change set is taken up again, and
  `building` is the build and test run in place of a compiler or as the unit gate.

## Rust: `..Default::default()`

- **Adopted as guidance of the `rust` layer** in `vendor/rust`, so every Rust project gets it. A
  struct whose field types all implement `Default` derives it. Code that builds a value of a struct
  another unit outputs ends the expression with `..Default::default()` after the fields it sets,
  unless it sets every field, which clippy's `needless_update` rejects under `-D warnings`.
- A new field whose type has a `Default` then breaks no dependent, so most data changes need no
  foreign edit at all. It does not help a field whose type has no `Default`, or a field removed or
  renamed; those take a fix or a stub.
