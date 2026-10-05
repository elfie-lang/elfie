# Writing Elfie

Elfie treats the LLM as a compiler. You write a deterministic, structured definition; the compiler generates code in a target language that must satisfy it. Write the contract, not the implementation.

Every entity has three layers: value (`x`, `x.member`), context (`x@type`, `x@acceptanceCriteria`), and scope (`x$member`; `$&` for the parent scope).

## Ground truth

- DO get exact syntax from the grammar (`elfie_grammar` MCP tool) and command usage from `elfie help`; they outrank this skill and your memory.
- DO read the files you edit and the files they `use`; match their idiom.
- DON'T copy syntax from old examples or other languages (`key: value` objects, `import`, `extends` on a `d` after the description).

## Framing

- DO describe what a consumer can observe: inputs, outputs, guarantees, side effects.
- DON'T describe internals (caches, stacks, helper steps, algorithms) or expose them as a `d` or `fn`.
- DO prefer interfaces: give a `d` the members consumers read, and a `fn` the parameters callers pass.
- DO state guarantees without saying how (`Every call ends`, `Never returns a duplicate`).
- DO let the caller say what it wants (a parameter naming the goal) instead of a mode flag the callee must interpret.
- DO expose one entry point per concern; split only when a consumer needs the split.
- DON'T create a `fn` that only another `fn` in the same file calls.
- DO write prose as plain, checkable statements; one fact per sentence.

## Choosing a construct

- DO use `d` for data and `fn` for behavior whose implementation the compiler generates from criteria. Prefer these.
- DO use `function` (written in full, `->` return) only when exact logic matters or for compile-time helpers.
- DO use `type` for a structural shape, `enum` for a closed set of named values, `alias` for a new name of an existing thing, `external` for something defined outside the program.
- DO use `trait` for anything shared across entities: members, criteria, rules. Compose with `is a, b`; inherit with `extends`.
- DO use `ace` to run a statement at compile time (lookup tables, helpers that build prose or grammar).
- DO `use "path";` for declarations and `use "path" as M;` for dotted access. DON'T reference a name from a file you did not `use`.

## Syntax essentials

```lfy
use "./traits";

enum Status: `Where an order is` {
  open = `Not yet paid`,
  paid = `Paid and awaiting shipment`,
}

d Order is persisted, owned(User): `A customer's order` {
  $id: `Unique and never changes` = string;
  $status: `Current state` = Status;
  .status = Status.open;

  $id@acceptanceCriteria.add({ behavior = `Is unique across all [[Order]]` });
}

fn placeOrder(user: User, items: Item[]) is api(1): `Creates an [[Order]] for [[user]]` => Order {
  @acceptanceCriteria
    .add({ behavior = `Returns an [[Order]] whose [[Order.status]] is open` })
    .add({ situation = `[[items]] is empty`, behavior = `Fails and creates nothing` });
}
```

- DO give every `d`, `fn`, `function`, `trait`, `type`, `enum`, member, and parameter worth explaining a description: `: \`text\``after the name,`is`, and `extends`, before `=>` or the body.
- DO declare members as `$name: \`description\` = Type;`and set values only where needed as`.name = value;`.
- DO write object fields as `key = value`. `:` attaches a description or type, never an object value.
- DO end expression statements, `where` lines, setters, and variable declarations with `;`. DON'T put `;` after a block.
- DON'T use a keyword as a name.
- DO use backtick templates for prose: `[[Name]]`, `[[Type.member]]`, `[[param]]` reference declared things; `{{expr}}` inserts a value; `{{@identifier}}` is the current entity's name; `[[&param]]` is the entity a parameter is bound to.
- DO use `///` or `/** **/` for documentation and `//` for notes to readers that the compiler should not treat as spec.

## Traits

- DO name traits so `X is <trait>` reads as a property: `persisted`, `documented`, `owned(User)`, `triedBefore(other)`.
- DON'T name traits as verbs or imperatives (`persists`, `binds`, `uses`).
- DO make marker traits parameterless, and take a single value when every application passes one.
- DO use every parameter in a member, setter, `where`, or criterion. A parameter only in a description is a defect.
- DO write rules as `where (\`a\`) and !(\`b\`) -> \`behavior\`;`. Wrap them in `with Target { ... }` when they act on another entity.
- DO apply a trait from outside an entity with `trait.apply(Entity, args);`, and iterate entities with `trait@entities`.
- DON'T apply the same trait twice to one entity.

## Acceptance criteria

- DO put one behavior per `.add({ ... })`; use `behavior = [ ... ]` only for tightly related facts.
- DO move every "when", "if", "unless", "after", "once" clause into `situation` (a string or a list, all of which must hold).
- DO make each criterion checkable: name the member, value, or output it constrains.
- DO make each criterion independent of the others; DON'T rely on order or presense of other criterion.
- DON'T join behaviors with `;` or "and also" in one string.
- DON'T restate a fact that a type, a trait, or another criterion already guarantees.
- DO put a criterion on the narrowest entity it constrains (a member's own `@acceptanceCriteria` over its owner's).

## Tests

- DO add `@test({ input = [args], expect = value }, ...)` for concrete cases that clarify behavior: edge cases, ties, errors.
- DO describe complex values in prose with `Type@like(\`...\`)`. DON'T write code-like dumps in `expect`.
- DON'T test what a criterion already states plainly.

## CLI

- `elfie init [name]` creates `elfie.json` and the source directory.
- `elfie check [files]` reports problems and stale units. Use `--strict` to fail on warnings and anything stale.
- `elfie format [files]` rewrites files; `--check` only reports.
- `elfie tree <file>` and `elfie tokens <file>` show how a file parses; use them when syntax surprises you.
- `elfie compile [units]` generates outputs for what changed. `--dry-run` shows the plan; `--all` forces every unit; `--target` limits to one.
- `elfie verify [units]` reviews generated output against its criteria without compiling.
- `elfie lsp` and `elfie mcp` serve editors and agents.
- DO add `--json` when a script or agent reads the output.

## MCP

- DO use `elfie_problems` after every edit and fix everything it reports.
- DO use `elfie_find`, `elfie_entity`, `elfie_references`, and `elfie_outline` to read the program instead of grepping.
- DO use `elfie_units`, `elfie_request`, `elfie_check`, and `elfie_review` / `elfie_globalReview` when acting as the compiler or verifier.
- DO use `elfie_output`, `elfie_source`, and `elfie_changes` to trace between definitions and generated code.

## Before finishing

- DO run `elfie check` (or `elfie_problems`) and `elfie format --check` on what you changed; leave nothing reported.
- DO confirm every `[[reference]]` names something declared or `use`d.
- DON'T edit generated output by hand. Change the definition and recompile.
