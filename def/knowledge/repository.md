# This repository: facts a compiler would otherwise rediscover

## The Cargo workspace

- The workspace manifest is `Cargo.toml` at the repository root. `crates/` holds no manifest of its
  own; each crate has its own `Cargo.toml`.
- The members, by directory and by **package name**:

  | Directory           | Package      | Kind                     |
  | ------------------- | ------------ | ------------------------ |
  | `crates/elfie-core` | `elfie-core` | library                  |
  | `crates/elfie-lsp`  | `elfie-lsp`  | library                  |
  | `crates/elfie-mcp`  | `elfie-mcp`  | library                  |
  | `crates/elfie-cli`  | `elfie`      | binary (`src/main.rs`)   |

- The CLI's package is `elfie`, not `elfie-cli`. Build or test it with `cargo build -p elfie` and
  `cargo test -p elfie`; `-p elfie-cli` names no package.
- The agent server's command tools (`elfie_build`, `elfie_test`, `elfie_lint`) take `scope` as one
  of the package names above, passed to cargo as `-p`: `elfie-core`, `elfie-lsp`, `elfie-mcp`, or
  `elfie`. Leaving `scope` out runs the whole workspace.

## Formatting

- `cargo fmt` is never run. The target declares a format command with no line, so it has none, and
  no `elfie_format` tool is offered. Committed code is not rustfmt-clean, and running it rewrites
  every unit's outputs, including other units' work.

## Shared outputs

- `crates/elfie-core/src/lib.rs` is an output of several units, and so are some module files
  (`mod.rs` files and a few others under `crates/elfie-core/src`).
- A unit adds only its own items and its own `pub mod` lines to such a file. It never removes,
  reorders, or rewrites another unit's marked regions, and it leaves the `pub mod` lines of other
  units in place.

## Tests that load the repository

- A generated test that loads the real repository (`def/`, `lib/`, `vendor/`) asserts properties
  that hold whatever the definitions say: that the load reports no problem, that each unit's stem is
  unique, that an entity the test names exists.
- It never pins a count or an order taken from the current definitions, such as the number of
  units, criteria, or files, or which unit comes first. Such a test fails as soon as a definition
  changes, in a unit nobody is compiling.
