#!/usr/bin/env bash
# The compiler command `elfie compile` runs once per unit: Claude Code, given the request on
# standard input, with the Elfie agent server and permission to edit files, to build and test
# through the server's command tools or cargo, and to run cargo add.
# ELFIE_ROOT, ELFIE_UNIT, ELFIE_MCP_CONFIG, and ELFIE_COMMAND_TOOLS are set by the caller; the
# working directory is the unit's own copy of the root, made for it in the compile's change set,
# or the root when run by hand. ELFIE_UNIT is the stem of the one unit the run is for, such as workspace/data.
# ELFIE_MCP_CONFIG names the config of the session the compile hosts for this run; without it, as
# when run by hand, the agent server is the one .mcp.json starts.
set -e
# The agent server's tools are loaded up front rather than deferred behind a tool search, and no
# other MCP server is started, so the elfie tools are the first thing the agent sees to reach for.
export ENABLE_TOOL_SEARCH=false
# claude -p starts MCP servers without waiting; wait for the agent server so its tools are there
# from the first turn.
export CLAUDE_CODE_MCP_PREWAIT_SERVERS=elfie
export CLAUDE_CODE_MCP_PREWAIT_SERVERS_MS=30000
# A command tool may wait in its directory's queue and then run a build or a test for up to 30
# minutes before the server stops it, so a tool call is given 40 minutes rather than the default.
export MCP_TOOL_TIMEOUT=2400000
# Definitions are read only through the agent server, which answers for an entity, a name, or a
# file at once; reading or searching def/ and lib/ by hand is denied. Generated code is read through
# elfie_output and elfie_search, but stays open to Read and Grep until the share of calls that go to
# the server is measured (ROADMAP 11).
# ELFIE_COMMAND_TOOLS names the command tools the hosted session offers, such as
# `elfie_build elfie_test elfie_lint`: each runs the target's command in this run's directory, one
# at a time. When it names any, cargo build, test, and clippy are not allowed through Bash; when it
# is empty or unset, as with an older binary or a run by hand, they are, as before. cargo add is
# allowed either way, because a dependency is added only by the target's add command and no tool
# runs it. Bash itself is never put in --disallowedTools, since a deny rule would also block cargo
# add; under -p a Bash command no allow rule matches is denied, because no one can be asked.
if [ -n "${ELFIE_COMMAND_TOOLS:-}" ]; then
  allowed="Read Edit Write Glob Grep Bash(cargo add:*) mcp__elfie__*"
else
  allowed="Read Edit Write Glob Grep Bash(cargo build:*) Bash(cargo test:*) Bash(cargo clippy:*) Bash(cargo add:*) mcp__elfie__*"
fi
# ELFIE_SESSION is the agent session id the compile gives this run, and ELFIE_RESUME, set only on a
# retry after a rejected outcome or a review that found a violation, the session of the compiler's
# run before it. Resuming keeps what the first attempt already read, so the retry does not read it
# all again; the retry's request, with the problems appended, is still given on standard input. A
# resumed session keeps its id, which is why the compile passes the same ELFIE_RESUME to every
# later retry. The session is resumed only when its file is in Claude Code's project store and is
# under 20 MB; a missing file (the run before died first) or a larger one (too long to continue)
# falls back to a fresh session under ELFIE_SESSION. With neither set, as when run by hand, Claude
# Code starts a session of its own.
store="${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects"
session=()
if [ -n "${ELFIE_RESUME:-}" ] && f=$(find "$store" -maxdepth 2 -name "$ELFIE_RESUME.jsonl" -size -20M -print -quit 2>/dev/null) && [ -n "$f" ]; then
  session=(--resume "$ELFIE_RESUME")
elif [ -n "${ELFIE_SESSION:-}" ]; then
  session=(--session-id "$ELFIE_SESSION")
fi
echo "session: ${session[*]:-none}" >&2
# ELFIE_COMPILER_MODEL, an operator's setting, picks the model, as --model takes it (an alias such
# as opus or sonnet, or a full model name); unset or empty, Claude Code's default is used.
model=()
if [ -n "${ELFIE_COMPILER_MODEL:-}" ]; then
  model=(--model "$ELFIE_COMPILER_MODEL")
fi
echo "compiling unit ${ELFIE_UNIT:-?} with Claude Code" >&2
exec claude -p \
  "${session[@]}" "${model[@]}" \
  --permission-mode acceptEdits \
  --allowedTools "$allowed" \
  --disallowedTools "Read(./def/**)" "Read(./lib/**)" \
  --strict-mcp-config --mcp-config "${ELFIE_MCP_CONFIG:-.mcp.json}" \
  --append-system-prompt "$(cat scripts/compiler-prompt.md)"
