#!/usr/bin/env bash
# Checks the definitions with the compiled compiler: every file binds with no problems and every
# unit's outputs are up to date.
cd "$(dirname "$0")/.."
set -e
cargo build -q -p elfie
./target/debug/elfie check --strict
