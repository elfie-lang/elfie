#!/usr/bin/env bash
# The verifier command `elfie compile` and `elfie verify` run once per batch: Claude Code, given
# the review request on standard input, with the Elfie agent server and permission to read and to
# run cargo, never to edit or write. ELFIE_ROOT, ELFIE_BATCH, and ELFIE_UNITS are set by the
# caller; the root is the working directory. Its standard output is the report the caller parses.
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
echo "verifying batch ${ELFIE_BATCH:-?} (${ELFIE_UNITS:-?}) with Claude Code" >&2
exec claude -p \
  --permission-mode default \
  --allowedTools "Read Glob Grep Bash(cargo test:*) Bash(cargo build:*) mcp__elfie__*" \
  --disallowedTools "Read(./def/**)" "Read(./lib/**)" \
  --strict-mcp-config --mcp-config .mcp.json \
  --append-system-prompt "$(cat scripts/verifier-prompt.md)"
