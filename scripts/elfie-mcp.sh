#!/usr/bin/env bash
# Starts the Elfie agent server for the project at the repository root, preferring the binary
# that is compiling (ELFIE, set by elfie compile), then a built binary, over a cargo run, so that
# startup is fast, never touches the build lock, and works in a batch's copy that has no target.
# Its output passes through mcp-compat.py, which adds the tools/list fields newer clients require.
cd "$(dirname "$0")/.."
if [ -n "$ELFIE" ] && [ -x "$ELFIE" ]; then exec python3 -I scripts/mcp-compat.py "$ELFIE" mcp --root .; fi
if [ -x target/release/elfie ]; then exec python3 -I scripts/mcp-compat.py target/release/elfie mcp --root .; fi
if [ -x target/debug/elfie ]; then exec python3 -I scripts/mcp-compat.py target/debug/elfie mcp --root .; fi
exec python3 -I scripts/mcp-compat.py cargo run -q -p elfie -- mcp --root .
