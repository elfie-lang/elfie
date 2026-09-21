---
name: writing-lfy
description: Rules for writing Elfie (.lfy) definition files: module ownership, allowed constructs, traits, agentic d and fn, acceptance criteria, tests, naming, and checks. Load before writing or reviewing anything under def/.
disable-model-invocation: true
---

# Writing .lfy

## Read first

- DO read `README.md` (value `.`, context `@`, scope `$` layers) before touching `def/`.
- DO read the module you are editing and the modules it uses; nothing else.

## Allowed constructs

- DO use only constructs that already appear in the defined grammar unless explicitly asked otherwise.
- DON'T use a keyword as a parameter or variable name (`trait`, `type`, `return`).
- DON'T end a `d ... {}` block with `;`.
- DON'T omit `;` after an expression statement, a `where` line, or a `.x = value` setter.

## Module ownership

- DO keep the grammar to facts: rules, terminals, categories, binding power, meaning-changing disambiguation.
- DO keep lexer strategy (modes, tie-breaks, candidate conditions) in the lexer, attached with `trait.apply` from `modes.lfy`.
- DO keep parser strategy (trivia, attempt order, recovery) in the parser, attached from `components.lfy`.
- DO keep names, scopes, trait application, and criteria collection in the model.
- DON'T put attempt order, priority numbers, or `$firstTokens` in the grammar.
- DON'T put lexer modes or parser recovery on grammar rules from inside grammar files.
- DON'T have the parser mark names; name finding is name resolution.
- DON'T `use` a later stage from an earlier one (grammar never uses parser or model).
- DO iterate `trait@entities` (`terminal@entities`, `statement@entities`) instead of listing modules.

## Files per module

- DO use `data.lfy` (what the module produces), `traits.lfy` (components it attaches), `components.lfy` or `modes.lfy` (only `apply` lines and loops), `main.lfy` (functions).
- DO `use` every file whose names you reference; `use "x" as M` for dotted access only.
- DON'T reference a name from a file you did not `use`.

## Declarations

- DO write `d Name is trait(args), other: `description` { ... }`.
- DO declare specific and key members as `$name: `description` = Type;` and, only where applicable, set values as `.name = value;`.
- DO put the description after `:` and before the body; put `extends` before `:`.
- DO give every `d`, `trait`, `fn`, and rule a description.
- DON'T declare the same name twice across a module.
- DON'T give a `d` every field the implementation might want or detail the internal machinery; give what consumers read.

## Traits

- DO name traits so `X is <trait>` reads as a property: `trivia`, `documented`, `recoverable`, `binding(...)`, `triedBefore(...)`, `matchedWhen(...)`, `opening(...)`, `reading(...)`, `declaring(...)`.
- DON'T name traits as third-person verbs or imperatives: `recovers`, `binds`, `opens`, `uses`, `declares`.
- DO use every parameter in a `where` line or a member setter; a parameter used only in prose is a defect.
- DO take a single value when every application passes one (`triedBefore(other)`), not a spread.
- DO write conditions as `const name = `...`;` and rules as `where (a) and !(b) -> `...`;` inside `with Target { }` when they act on another entity.
- DO make marker traits parameterless (`trivia`, `documented`, `scoped`).
- DON'T apply the same trait twice to one entity with different arguments.
- DON'T restate a grammar fact in a component; reference the rule instead.
- DO derive syntax in a trait when it is mechanical (`prefix(operator)` builds `[[&operator]] , [[Expression]]`).

## Agentic d and fn

- DO define `fn` by inputs, outputs, and observable outcomes only.
- DON'T describe internal machinery (frames, memo keys, stacks) in an `fn` or expose it as a `d`.
- DON'T create a `fn` for internal machinery or logic that is not observable to a consumer.
- DON'T create a `fn` that is meant to be called by another `fn` in the same module.
- DO expose at least one entry point per module (`parse(tokens, rule = SourceFile) => Tree`); split only when a consumer needs the split now.
- DO let the caller name what it wants (a rule to satisfy) instead of a mode flag the callee must guess from.
- DO state guarantees a caller can rely on (`Every parse ends`) without saying how.

## Acceptance criteria

- DO add one behavior per `.add` for most cases; use `behavior = [ ... ]` for several related facts.
- DO move every "when", "where", "unless", "after", "once" clause into `situation`.
- DON'T join behaviors with `;` or "and also" inside one string.
- DON'T put `expect` in `@acceptanceCriteria` for concrete outputs; use `@test`.
- DO make each criterion checkable: name the node, token, index, or member it changes.
- DO reference declared things with `[[Name]]`, members with `[[Type.member]]`, parameters with `[[param]]`, and the current entity with `{{@identifier}}`.
- DON'T reference a trait or rule that no `use`d file declares.
- DO state grammar disambiguation that changes meaning on the grammar rule, and parser strategy on the parser.

## Tests

- DO write a limited number of `@test({ input = [args], expect = value }, ...)` for concrete cases to clarify behavior.
- DON'T write tests for things covered by acceptance criteria unless it is an edge case, or critical to the functionality to get correct.
- DO describe inputs and outputs with `Type@like(`prose`)` (`Token[]@like(`The lexing of: a - b`)`, `Tree@like(`An [[AdditiveOperation]] root ...`)`).
- DON'T write code-like tree or token notation in `expect`.
- DO cover: empty input, one case per selection tie, one per precedence rule, one error recovery case.

## Grammar specifics

- DO put binding power on operator tokens and per-level operator lists; override on a rule only where one token has two roles.
- DO keep one precedence table (`enum Level`).
- DO make category lists `alternationList(...category@entities)`; order carries no meaning.
- DO give every statement and expression rule exactly one category.
- DON'T reference a rule from `[[...]]` in EBNF unless the identifier exists somewhere in the grammar.

## Checks before finishing

- DO run the reference checker: no dangling `[[Name]]`, no undeclared trait in `is`/`extends`/`.apply`, no duplicate declaration, no unresolved `use` path.
- DO walk one real file by hand against the grammar after grammar changes.
- DON'T commit unless asked.
