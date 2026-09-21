#!/usr/bin/env bash
# The verifier command `elfie compile` and `elfie verify` run once per batch: Claude Code, given
# the review request on standard input, with the Elfie agent server and permission to read and to
# run cargo, never to edit or write. ELFIE_ROOT, ELFIE_BATCH, and ELFIE_UNITS are set by the
# caller; the root is the working directory. Its standard output is the report the caller parses.
set -e
echo "verifying batch ${ELFIE_BATCH:-?} (${ELFIE_UNITS:-?}) with Claude Code" >&2
exec claude -p \
  --permission-mode default \
  --allowedTools "Read Glob Grep Bash(cargo test:*) Bash(cargo build:*) mcp__elfie__*" \
  --mcp-config .mcp.json \
  --append-system-prompt "$(cat scripts/verifier-prompt.md)"
