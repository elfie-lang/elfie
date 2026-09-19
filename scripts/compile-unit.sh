#!/usr/bin/env bash
# The compiler command `elfie compile` runs once per unit: Claude Code, given the request on
# standard input, with the Elfie agent server and permission to edit files and run cargo.
# ELFIE_ROOT and ELFIE_UNIT are set by the caller; the root is the working directory.
set -e
echo "compiling ${ELFIE_UNIT:-?} with Claude Code" >&2
exec claude -p \
  --permission-mode acceptEdits \
  --allowedTools "Read Edit Write Glob Grep Bash(cargo build:*) Bash(cargo test:*) Bash(cargo clippy:*) Bash(cargo fmt:*) mcp__elfie__*" \
  --mcp-config .mcp.json \
  --append-system-prompt "$(cat scripts/compiler-prompt.md)"
