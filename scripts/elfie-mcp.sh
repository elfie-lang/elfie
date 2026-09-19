#!/usr/bin/env bash
# Starts the Elfie agent server for the project at the repository root, preferring a built
# binary over a cargo run so that startup is fast and never touches the build lock.
cd "$(dirname "$0")/.."
if [ -x target/release/elfie ]; then exec target/release/elfie mcp --root .; fi
if [ -x target/debug/elfie ]; then exec target/debug/elfie mcp --root .; fi
exec cargo run -q -p elfie -- mcp --root .
