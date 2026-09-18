#!/usr/bin/env bash
# Checks the definitions with the compiled compiler: every def file parses, the program binds
# with no problems, every unit's outputs are up to date, and the reference checker agrees.
cd "$(dirname "$0")/.."
set -e
cargo build -q -p elfie
./target/debug/elfie check
./target/debug/elfie compile --dry-run | awk '$3 != "up" { print "stale: " $0; bad = 1 } END { exit bad }'
python3 scripts/refcheck.py
