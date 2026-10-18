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

## Platforms

- The release builds `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
  `aarch64-apple-darwin`, and `x86_64-pc-windows-msvc`. Code that calls a platform API compiles on
  every one of them: an API of one platform family sits under `#[cfg(unix)]` or `#[cfg(windows)]`,
  with a branch for the other.
- Check the Windows build with `cargo check --target x86_64-pc-windows-msvc --workspace`.
  `cargo build` and `cargo test` on Linux never compile a `#[cfg(windows)]` branch.
- The operating system's secure random source is `getrandom::fill`, which works on every one of
  them. `/dev/urandom` does not exist on Windows.
- A process group on Windows is a process started with the `CREATE_NEW_PROCESS_GROUP` creation
  flag, through `std::os::windows::process::CommandExt::creation_flags`. It and every process it
  started are stopped with `taskkill /T /F /PID <id>`. On Unix, `process_group(0)` and
  `kill -KILL -- -<id>` do the same.
- The platform shell is `sh -c` on Unix and `cmd /C` on Windows.

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

## A change that breaks a later unit's code

- Units are compiled one at a time in plan order, so a unit that adds, removes, or renames a member
  of a data, or changes behavior a later unit's code or fixtures rely on, is compiled before the
  units that construct or read it. It writes the output its definition asks for anyway; it does not
  stop as blocked.
- The whole workspace must build, and every test pass or fail only by reaching a stub, before the
  unit is accepted. So the unit repairs each site it breaks in a later unit's code itself, inside
  that unit's regions: a fix within the cap `elfie_reach` reports, with a note through
  `elfie_touch` for each unit it fixes, or a stub when the change is bigger. Those units are
  compiled later in the same compile, planned as touched.
- A struct expression of a struct that derives `Default` and ends with `..Default::default()`
  needs no edit when a field whose type has a `Default` is added, so most added fields break
  nothing.
- A broken site is never patched temporarily and restored: what the unit leaves in a later unit's
  code is what is accepted and recorded.

## Tests that load the repository

- A generated test that loads the real repository (`def/`, `lib/`, `vendor/`) asserts properties
  that hold whatever the definitions say: that the load reports no problem, that each unit's stem is
  unique, that an entity the test names exists.
- It never pins a count or an order taken from the current definitions, such as the number of
  units, criteria, or files, or which unit comes first. Such a test fails as soon as a definition
  changes, in a unit nobody is compiling.
