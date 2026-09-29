#!/usr/bin/env bash
# Computes the version a release channel publishes next, and refuses one that breaks the layer
# rules. Usage: scripts/release/version.sh <agentic|beta|rc|final> [base]
#
# The base (X.Y.Z) is the [workspace.package] version in Cargo.toml unless given. The stable
# baseline is the highest vX.Y.Z tag, or 0.0.0 before the first release. A base must be above
# the baseline and at most one whole version above it: the next minor while the baseline is
# below 1.0.0 (plus 1.0.0 itself), the next major from 1.0.0 on.
#
# agentic, beta, and rc publish <base>-<channel>.N, where N is one past the highest existing
# v<base>-<channel>.N tag. final publishes <base> from the commit of the highest v<base>-rc.N tag.
#
# Prints the version, and when GITHUB_OUTPUT is set also writes version, tag, prerelease (true
# or false), and sha (the commit to build) to it.
set -euo pipefail
cd "$(dirname "$0")/../.."

fail() { echo "error: $*" >&2; exit 1; }

channel=${1:-}
case "$channel" in
    agentic | beta | rc | final) ;;
    *) fail "usage: $0 <agentic|beta|rc|final> [base]" ;;
esac

base=${2:-$(awk '/^\[workspace\.package\]/ { inside = 1; next } /^\[/ { inside = 0 } inside && /^version *=/ { gsub(/[" ]/, "", $0); sub(/^version=/, "", $0); print; exit }' Cargo.toml)}
[[ $base =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] \
    || fail "base version '$base' is not a plain X.Y.Z"
IFS=. read -r major minor patch <<< "$base"

# Compares two X.Y.Z versions; prints -1, 0, or 1.
compare() {
    local a b i
    IFS=. read -ra a <<< "$1"
    IFS=. read -ra b <<< "$2"
    for i in 0 1 2; do
        if ((a[i] < b[i])); then echo -1; return; fi
        if ((a[i] > b[i])); then echo 1; return; fi
    done
    echo 0
}

stable=$(git tag --list 'v*' | sed -n -E 's/^v([0-9]+\.[0-9]+\.[0-9]+)$/\1/p' | sort -V | tail -n 1)
stable=${stable:-0.0.0}
IFS=. read -r stable_major stable_minor _ <<< "$stable"

[[ $(compare "$base" "$stable") == 1 ]] \
    || fail "base $base is not above the latest stable release $stable; bump the version in Cargo.toml"

if ((stable_major == 0)); then
    cap="0.$((stable_minor + 1)).x"
    ((major == 0 && minor <= stable_minor + 1)) || [[ $base == 1.0.0 ]] \
        || fail "base $base is more than one minor version above the latest stable release $stable (at most $cap or 1.0.0)"
else
    cap="$((stable_major + 1)).x.x"
    ((major <= stable_major + 1)) \
        || fail "base $base is more than one major version above the latest stable release $stable (at most $cap)"
fi

# The highest N among v<base>-<label>.N tags, or 0 when there are none.
highest() {
    git tag --list "v$base-$1.*" | sed -n -E "s/^v${base//./\\.}-$1\.([0-9]+)$/\1/p" | sort -n | tail -n 1
}

if [[ $channel == final ]]; then
    rc=$(highest rc)
    [[ -n $rc ]] || fail "there is no v$base-rc.N release candidate to accept"
    version=$base
    prerelease=false
    sha=$(git rev-list -n 1 "v$base-rc.$rc")
else
    n=$(highest "$channel")
    version="$base-$channel.$((${n:-0} + 1))"
    prerelease=true
    sha=$(git rev-parse HEAD)
fi

echo "$version"
if [[ -n ${GITHUB_OUTPUT:-} ]]; then
    {
        echo "version=$version"
        echo "tag=v$version"
        echo "prerelease=$prerelease"
        echo "sha=$sha"
    } >> "$GITHUB_OUTPUT"
fi
