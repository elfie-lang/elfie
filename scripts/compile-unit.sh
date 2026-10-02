#!/usr/bin/env bash
# The compiler command `elfie compile` runs once per unit: Claude Code, given the request on
# standard input, with the Elfie agent server and permission to edit files and run cargo.
# ELFIE_ROOT and ELFIE_UNIT are set by the caller; the root is the working directory.
set -e
# The agent server's tools are loaded up front rather than deferred behind a tool search, and no
# other MCP server is started, so the elfie tools are the first thing the agent sees to reach for.
export ENABLE_TOOL_SEARCH=false
# Definitions are read only through the agent server, which answers for an entity, a name, or a
# file at once; reading or searching def/ and lib/ by hand is denied. Generated code stays open to
# Read and Grep, for the helpers no marker names.
echo "compiling batch ${ELFIE_BATCH:-?} (${ELFIE_UNITS:-?}) with Claude Code" >&2
exec claude -p \
  --permission-mode acceptEdits \
  --allowedTools "Read Edit Write Glob Grep Bash(cargo build:*) Bash(cargo test:*) Bash(cargo clippy:*) mcp__elfie__*" \
  --disallowedTools "Read(./def/**)" "Read(./lib/**)" \
  --strict-mcp-config --mcp-config .mcp.json \
  --append-system-prompt "$(cat scripts/compiler-prompt.md)"
