#!/usr/bin/env bash
# The compiler command `elfie compile` runs once per unit: Claude Code, given the request on
# standard input, with the Elfie agent server and permission to edit files and run cargo.
# ELFIE_ROOT, ELFIE_BATCH, ELFIE_UNITS, and ELFIE_MCP_CONFIG are set by the caller; the root is the
# working directory. ELFIE_MCP_CONFIG names the config of the session the compile hosts for this
# run; without it, as when run by hand, the agent server is the one .mcp.json starts.
set -e
# The agent server's tools are loaded up front rather than deferred behind a tool search, and no
# other MCP server is started, so the elfie tools are the first thing the agent sees to reach for.
export ENABLE_TOOL_SEARCH=false
# claude -p starts MCP servers without waiting; wait for the agent server so its tools are there
# from the first turn.
export CLAUDE_CODE_MCP_PREWAIT_SERVERS=elfie
export CLAUDE_CODE_MCP_PREWAIT_SERVERS_MS=30000
# Definitions are read only through the agent server, which answers for an entity, a name, or a
# file at once; reading or searching def/ and lib/ by hand is denied. Generated code stays open to
# Read and Grep, for the helpers no marker names.
# In a session the compile hosts, the target's build, test, and lint commands are tools of the
# agent server (elfie_build, elfie_test, elfie_lint), run in this run's directory one at a time, so
# no shell is allowed at all. Without one, as with an older binary or a run by hand, cargo is run
# through Bash as before.
if [ -n "${ELFIE_MCP_CONFIG:-}" ]; then
  allowed="Read Edit Write Glob Grep mcp__elfie__*"
  denied="Bash"
else
  allowed="Read Edit Write Glob Grep Bash(cargo build:*) Bash(cargo test:*) Bash(cargo clippy:*) Bash(cargo add:*) mcp__elfie__*"
  denied=""
fi
echo "compiling batch ${ELFIE_BATCH:-?} (${ELFIE_UNITS:-?}) with Claude Code" >&2
exec claude -p \
  --permission-mode acceptEdits \
  --allowedTools "$allowed" \
  --disallowedTools "Read(./def/**)" "Read(./lib/**)" ${denied:+"$denied"} \
  --strict-mcp-config --mcp-config "${ELFIE_MCP_CONFIG:-.mcp.json}" \
  --append-system-prompt "$(cat scripts/compiler-prompt.md)"
