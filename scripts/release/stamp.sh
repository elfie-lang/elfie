#!/usr/bin/env bash
# Writes a release version into the workspace so the build reports it: the [workspace.package]
# version in Cargo.toml and the workspace members in Cargo.lock. The change is never committed.
# Usage: scripts/release/stamp.sh <version>
set -euo pipefail
cd "$(dirname "$0")/../.."

version=${1:?usage: $0 <version>}
awk -v version="$version" '
    /^\[/ { inside = ($0 == "[workspace.package]") }
    inside && /^version *=/ { $0 = "version = \"" version "\"" }
    { print }
' Cargo.toml > Cargo.toml.stamped
mv Cargo.toml.stamped Cargo.toml
cargo update --workspace --quiet
