# Elfie roadmap: today to v0.5

This document says where the language is, what each release from v0.1 to v0.5 is for, and what
goes in each. It is deliberately more exact for v0.1 than for v0.5: v0.1 is a checklist, v0.2 is
a set of committed items plus candidates, v0.3 is themes with open questions, and v0.4 and v0.5
are directions with the signals that will tell us we are there. Each release rewrites the section
after it into the shape the release before it had.

The roadmap lives at the repository root and is revised at every release. Decisions made along
the way keep going into `def/DECISIONS.md`; this file only says where they are heading.

## At a glance

| Track | Today | v0.1 | v0.2 | v0.3 | v0.4 | v0.5 |
| --- | --- | --- | --- | --- | --- | --- |
| Language | Three layers, criteria, tests, generics | Grammar frozen for the release, open issues closed | Errors, async, destructuring | Visibility, iterators, richer matching | Fills what real programs need | Spec, edition policy |
| Compilation | Everything through the agent | Deterministic and agentic entities named and reported | Deterministic translator is real; agent fills regions | Cached, reproducible, auditable | Fast enough for daily use | Stable contracts for targets and compilers |
| Interactive build | Question stops the compile, answered in a file | Questions and ambiguity are diagnostics | Watch mode; edits re-plan a running compile | Answers become code; build conversation is an artifact | Editor is the build console | Polished, documented |
| Packages | Local path dependencies | `@scope/name`, versions, git deps, lockfile | Registry index, `elfie publish` | Hosted registry, verified packages, shipped outputs | Package-level targets and tooling | Stability guarantees |
| Targets | Rust | Rust, cleaned up | TypeScript | One more, chosen by demand | Two or three more | Target authoring is documented and stable |
| Standard library | Prelude, values, file, path, process, json, time | Console, hash, Map/Set, math; compiled as a crate | Result, Promise, collections | Network, regex, streams | Broad enough for services and CLIs | Frozen surface for 1.0 |
| Tooling | CLI, LSP, MCP, VS Code | Installable binary, published extension, `elfie test` | `elfie add`, `elfie run`, watch | Debugger through source maps | Profiling, migration tools | Polish |

## Where Elfie is today

Elfie is a self-hosted language whose compiler is an agent. The definitions under `def/` are
the compiler; Claude Code compiles them to the Rust workspace under `crates/`, an independent
verifier judges every criterion and test, and the result is the `elfie` binary that checks,
formats, plans, and drives the next compile.

What exists and works:

- **The language.** Three layers (`.` value, `@` context, `$` scope), `d`/`fn` for generated
  declarations and `function` for written-out ones, traits with bodies that run at bind time,
  templates with `[[references]]` and `{{expressions}}`, acceptance criteria and tests as first
  class members of every entity, positional generics, and `@like` values described in prose.
- **The compiler.** 36 units compile in dependency order, batched, with staleness decided by
  interface hashes. Ambiguity is a compile error. Structural acceptance, then independent
  verification, then source maps that name entities rather than lines.
- **The tools.** A CLI (`check`, `format`, `tree`, `tokens`, `compile`, `verify`, `lsp`, `mcp`),
  a language server with hover, definition, references, symbols, rename, formatting, semantic
  tokens and diagnostics, an agent server with thirteen read-only tools, and a VS Code extension.
- **The standard library.** A prelude every file sees, value types, criteria and tests, and system
  modules for files, paths, processes, JSON, and time. One target, Rust, declared as a package.

What does not exist yet, and what the releases below are for:

- Nothing is compiled without the agent. A `function` body is "translated statement by
  statement", but by the agent. There is no deterministic code generator.
- Nobody outside this repository has run it. There is no installable binary, no tutorial, and the
  only project ever compiled is the compiler.
- Packages are local directories. There are no names, versions, installs, or registry.
- The agentic build is a batch job. A question stops it and is answered in a file.
- The language is missing what a general-purpose language needs: error handling, async
  semantics, destructuring, visibility, and a broader library.

## What holds through every release

These are the rules the roadmap is built on. A release that breaks one is a mistake, not a
trade-off.

1. **Ambiguity is a diagnostic, never a guess.** Every stage that cannot decide says so, at the
   entity, in the editor and on the command line. This is the product.
2. **Deterministic where it can be, agentic where it must be.** The line between the two is
   visible in the source and reported by the compiler. A program never pays the agent for
   something a translator can do.
3. **The definition is the truth.** Generated code is an output. Fixes go into criteria and
   tests, and the source map is how outputs are traced back. This holds for the compiler itself.
4. **A target is a package, not a change to the compiler.** Adding a language means writing
   guidance and bindings, not forking the toolchain.
5. **Verification is independent.** The agent that writes never judges its own work.

## v0.1: someone else can use it

**Goal.** A person who is not the author installs Elfie, writes a small program, compiles it to
Rust with the agent, gets clear errors when the definition is ambiguous, and edits in an editor
that understands the language. The compiler's own rules are settled enough to write down.

**Target date.** End of December 2026. This is a checklist release: it ships when the exit
criteria below hold, and nothing else moves the date.

### Language

1. Close the open grammar issues from `def/DECISIONS.md`: parameters carry a description after
   their type; a marker applies to a file entity and to `global` with one documented spelling;
   a keyword in name position gets its own diagnostic; `[[` inside code and the reference checker
   agree.
2. Define what the grammar already parses but nothing specifies: `async`/`await` (a `Promise<T>`
   in the library and a rule for when a function is async), `ace` (compile-time evaluation is
   restricted to what the binder can run today, and everything else is a diagnostic), and
   `matchall`.
3. Write down which declarations are deterministic and which are agentic (see Compilation). A
   `fn` is agentic. A `function`, `type`, `enum`, `alias`, `const`, `let`, and a `d` whose members
   carry no criteria and no bodies are deterministic. A `d` with criteria is both: its shape is
   deterministic and its behaviors are agentic.
4. Freeze the grammar for the release and generate the language reference from `def/grammar`.
   The EBNF is already produced by the definitions; v0.1 publishes it with one page per rule.
5. Decide, in an RFC and not in code, the error-handling model and the visibility model, so v0.2
   can implement them. The candidates are a `Result<T, E>` type with `?`-style propagation
   against `throw`, and `public` by default with `private` against export lists.

### Compilation

1. **Classification.** The planner marks every entity of a unit as deterministic or agentic and
   `elfie compile --dry-run` and `elfie_units` report the split. The request tells the agent which
   entities it may change. Nothing is translated deterministically yet; the boundary is made
   visible first so v0.2 can move work across it without changing what a program means.
2. **Close the binder gaps the library rewrite found.** Criteria written in `with X$member`
   attach to the member; `criteriaOf` includes extended traits' criteria; `Entity.sideEffects`
   has a criterion that says what writes it. The compiler's own definitions are the test.
3. **Bootstrap discipline.** A release is a tagged binary that compiles the definitions of the
   next release. `scripts/check.sh` becomes the release gate: every unit up to date, every
   review satisfied or unverifiable, no violated review.
4. **Diagnostics for ambiguity.** Blocked and clarification outcomes become diagnostics on the
   entity they concern, with a stage of `compiler`, surfaced by `elfie check` and the language
   server, and a question is answered by editing the definition. The question file under
   `elfie-requests/` stays as the record.
5. **The library compiles as a crate.** `lib/` becomes a unit set of its own, so a target binds
   builtins and links the rest instead of translating written-out members at every use.

### Packages

1. A package has a name of the form `@scope/name` and a version. `elfie` stays the one
   unscoped name, reserved for the standard library.
2. `elfie.json` dependencies accept a path, a git URL with a revision, or a version. Version
   resolution is exact-match in v0.1; ranges wait for the registry.
3. `elfie add @scope/name` fetches into a per-project cache and writes `elfie.lock`. Loading a
   package reads the lockfile and never the network.
4. A package's native dependencies are merged into the project's, and a conflict between two
   packages' pins is a load problem, not a guess.
5. A package declares which targets it supports by carrying their guidance; a project that
   compiles a package for a target it does not declare gets a diagnostic.

### Standard library

- Console and standard streams; a hash module (SHA-256 is prose today); `Map<K, V>` and
  `Set<T>`; math beyond `Number`'s members; `Promise<T>` with the semantics the language
  section defines.
- Every member of every library data has criteria, tests, and a native binding in the Rust
  target's guidance. A library member with no binding on a target is a diagnostic naming the
  target.

### Tooling

1. `cargo install elfie` and a release binary per platform, with the library bundled, so
   `ELFIE_LIB` is a development-only override.
2. `elfie init` produces a project that compiles: an `elfie.json` with the Rust target, one
   file, and a compiler configuration that runs the agent through the same wrapper this
   repository uses.
3. `elfie test` runs the generated tests of every unit through the target's test runner and
   prints failures as diagnostics at the definition's test, using the source map.
4. The VS Code extension is published, with code actions for the two things a person does most:
   answer a question, and add a criterion from a violated review.
5. A conformance suite under `tests/conformance/`: Elfie programs with expected tokens, trees,
   diagnostics, and (where deterministic) outputs, run by `scripts/check.sh`. This is what
   protects the language while the compiler is regenerated.

### Not in v0.1

The deterministic translator, a second target, the registry, watch mode, error handling and
visibility in the language, destructuring, and any change to the three-layer model.

### Exit criteria

- A person following the tutorial builds and runs a program of about a hundred lines with two
  `fn`s and one `d` with criteria, on a machine that has never seen this repository.
- The language reference, the tutorial, and the three-layer explanation are published at
  elfie.dev and generated or checked from the definitions.
- Every issue listed in `def/DECISIONS.md` as of this writing is closed or has a numbered
  successor in the tracker.
- The conformance suite has at least one program per grammar rule and per diagnostic kind.
- `elfie check` on a definition the agent could not compile shows the reason at the entity,
  in the editor, before any compile is run again.

## v0.2: two compilers, one language

**Goal.** Elfie compiles traditionally where it can. The deterministic parts of a program are
translated by the compiler in milliseconds; the agent receives only the regions it must write,
with the deterministic scaffolding already in place. A second target proves that a target is a
package. The language gains the features the v0.1 RFCs settled.

**Target window.** First half of 2027. The committed items ship; candidates move to v0.3 without
notice if they slow the committed ones.

### Committed

1. **The deterministic translator.** `function` bodies, expressions, control flow, `type`,
   `enum`, `alias`, `const`, `let`, and criteria-free `d` are emitted by the compiler itself,
   with markers, for each target from a target's binding table. The agent gets a request whose
   outputs already exist and whose agentic regions are named; it writes inside those regions
   only. Acceptance rejects a change outside them. A definition that uses only deterministic
   declarations compiles with no agent at all.
2. **TypeScript target.** A second target package, `@elfie/target-typescript`, with layout
   traits for one file per unit and a package per project, bindings for every builtin, and its
   own guidance. The compiler's own definitions are not required to compile to it; the
   conformance suite is.
3. **Error handling and visibility.** Whatever the v0.1 RFCs chose, implemented in the grammar,
   the binder, both targets, and the library. Diagnostics for an unhandled error and for a use
   of something not visible.
4. **Watch mode.** `elfie compile --watch` keeps a plan alive. An edit to a definition re-plans
   the units whose interface changed, and a unit whose batch is in flight gets the change
   appended to its request rather than restarting. Progress stays one line per event.
5. **Registry index and publish.** A git-hosted index in the shape the lockfile already reads,
   `elfie publish` that checks, formats, verifies, and pushes a package with its version, and
   version ranges in `elfie.json` resolved against the index into `elfie.lock`.

### Candidates

- Destructuring in `const`, `let`, parameters, and `for`.
- `match` with bindings and guards, so a union type can be taken apart without casts.
- Type inference beyond positional generics: return types, inline function parameters, and
  narrowing on `is` and equality.
- `elfie run` for the project's main entity on a target that can run it, and `elfie test` on
  the TypeScript target.
- Cost and duration of the agentic part reported per unit, so a person can see what a
  criterion costs.

### What we will learn here

Whether the boundary drawn in v0.1 is the right one. If real programs keep pushing bodies into
`fn` because `function` is too rigid, the language needs more expressiveness on the
deterministic side, and v0.3 leans that way. If they keep pushing into `function` because the
agent is slow or expensive, v0.3 leans toward caching and cheaper requests.

## v0.3: an ecosystem and a build you can talk to

**Goal.** Packages are published, found, installed, and trusted. The build is a conversation
recorded next to the code: a question the compiler asked and the answer a person gave are
artifacts, and an answer can become a criterion with one action. Agentic compilation is cheap
enough to run on every save because it is cached and reproducible.

**Target window.** Second half of 2027. Themes, not a checklist. Each theme gets its own
scoping document at the start of the release, in the style of `def/DECISIONS.md`.

### Themes

- **A hosted registry at elfie.dev.** Scoped names, ownership, semver, yanking, and a package
  page generated from the definitions: the context layer is the documentation. A published
  package is verified at publish time and carries the reviews.
- **Shipped outputs.** A package can publish its accepted outputs per target with their source
  maps and interface hashes, so a consumer whose target and interfaces match links them instead
  of compiling. This is the compile cache, made public.
- **The build conversation.** `elfie-requests/` becomes a `.elfie/` build directory with a
  history: every request, report, review, question, and answer, keyed by unit and interface
  hash. An answer to a question can be promoted to a criterion on the entity, with the same
  spelling the person would have written. A rejected review can be promoted to a test.
- **Reproducible agentic compilation.** A recorded request plus its accepted output is
  replayable: the same definition, target, and library version gives the same output without
  calling the agent. Drift between the recorded and the current definition is a diagnostic.
- **The language keeps growing toward general purpose.** Iterators and generators (`yield` is
  reserved), richer string and template handling, numeric literal forms, and the collection and
  network modules real services need.

### Open questions to settle before v0.3 starts

- Does a target's binding table live in the target package or in the library, per builtin?
- Who verifies a published package: the publisher's verifier, the registry, or the consumer at
  install? The likely answer is all three with different weights, but it needs a decision.
- Should `.elfie/` be committed? The source maps are; the conversation is larger.
- Is one agent harness enough, or does the compiler command need a small protocol of its own
  so any harness can implement it? The outcome lines already are one; the question is whether to
  document them as a contract.

## v0.4: general purpose

**Goal.** Elfie is a reasonable choice for a service, a CLI, a library, or a client, not only
for programs shaped like its own compiler. Most of a real program is deterministic, the agentic
parts are small and explicit, and the editor is where the build is watched and steered.

**Target window.** 2028. Directions, not commitments. The v0.3 release rewrites this section
into themes.

### Directions

- **More targets, chosen by demand.** Candidates are Python, Go, Swift and Kotlin for
  platform-native clients, and WebAssembly through the Rust target. The README's promise that
  one code base serves several interfaces and platforms is met here or not at all.
- **The editor as the build console.** Questions, reviews, progress, and cost appear at the
  entity in the editor; an answer typed there re-plans the unit; a generated region can be
  opened from its definition and the definition from the region.
- **Performance.** The deterministic translator, the binder, and the planner are fast enough for
  a large project on every keystroke; the agentic part is batched and cached so a change to one
  criterion costs one request.
- **Debugging through source maps.** A failing generated test or a runtime error is shown at the
  criterion it violates, on every target that can report a line.
- **Migration.** A tool that rewrites definitions across a grammar change, so language changes
  in v0.4 and v0.5 do not strand users.

## v0.5: something to stand on

**Goal.** The last release before stability is promised. Everything a 1.0 will guarantee is
written down, tried on real projects, and either kept or removed.

**Target window.** After v0.4 has been used for real work, and not before. This section is
goals and the signals that say we are there. Each earlier release rewrites it.

### Goals

- A language specification that stands apart from the compiler's definitions and is checked
  against them, with the three layers, criteria, tests, and the deterministic boundary as
  normative text.
- An edition policy: what a breaking change is, how one is announced, and how the migration tool
  carries a project across.
- A stable contract for target packages and compiler commands, so a target written for v0.5
  compiles on 1.0.
- A standard library whose surface is frozen, with every member bound on every supported target.
- A community process for language changes, in the shape of the RFCs v0.1 introduced.

### Signals that v0.5 is ready to be 1.0

- Two or more projects that are not the compiler are maintained on Elfie through a release
  cycle without a language change breaking them.
- The compiler's own definitions have been through a full deterministic-plus-agentic recompile
  with no violated reviews and no questions, from a tagged binary.
- A target package written by someone outside the core team compiles the conformance suite.
- The registry has packages from more than one publisher that depend on each other.

## How this roadmap changes

- **Each release rewrites the next section into the shape the current one had.** v0.1's
  release rewrites v0.2 into a checklist with exit criteria and v0.3 into committed items and
  candidates, and so on down.
- **Items move down, not up.** A v0.2 candidate can become a v0.3 theme without discussion. A
  v0.3 theme becomes a v0.2 commitment only at a release boundary, and only if a committed item
  moved down to make room.
- **Principles do not move.** A proposal that would break one is an RFC to change the
  principle first.
- **Dates are estimates that get looser with distance.** v0.1 has a month, v0.2 a half-year,
  v0.3 a half-year with less confidence, v0.4 a year, v0.5 none.
