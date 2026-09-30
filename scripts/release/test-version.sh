#!/usr/bin/env bash
# Checks version.sh against a scratch repository with made-up tags.
set -euo pipefail
script="$(cd "$(dirname "$0")" && pwd)/version.sh"
repo=$(mktemp -d)
trap 'rm -rf "$repo"' EXIT
cd "$repo"
git init -q
git -c user.name=test -c user.email=test@example.com commit -q --allow-empty -m first
mkdir -p scripts/release
cp "$script" scripts/release/version.sh
printf '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "0.1.0"\nedition = "2024"\n' > Cargo.toml

failures=0
# expect <want> <channel> [base]: want is a version, or "error" for a refused one.
expect() {
    local want=$1 got
    shift
    got=$(scripts/release/version.sh "$@" 2> /dev/null) || got=error
    if [[ $got != "$want" ]]; then
        echo "FAIL: version.sh $* gave $got, want $want"
        failures=$((failures + 1))
    fi
}
tag() { git tag "$1"; }

# Nothing released: the baseline is 0.0.0, so 0.1.x is the cap.
expect 0.1.0-experimental.1 experimental
expect 0.1.0-beta.1 beta
expect 0.1.0-rc.1 rc
expect error final
expect error experimental 0.2.0
expect 1.0.0-experimental.1 experimental 1.0.0
expect error experimental 1.0.1
expect error experimental 0.1
expect error experimental 0.1.0-beta.1
expect error nightly

# Counters are per base and per channel, and take the highest existing N.
tag v0.1.0-experimental.1
tag v0.1.0-experimental.9
tag v0.1.0-experimental.10
tag v0.1.0-beta.2
expect 0.1.0-experimental.11 experimental
expect 0.1.0-beta.3 beta
expect 0.1.0-rc.1 rc
expect 0.1.1-experimental.1 experimental 0.1.1

# final accepts the highest release candidate.
tag v0.1.0-rc.1
tag v0.1.0-rc.2
expect 0.1.0 final
[[ $(GITHUB_OUTPUT=/dev/stdout scripts/release/version.sh final | grep -c '^tag=v0.1.0$') == 1 ]] \
    || { echo "FAIL: final does not write tag=v0.1.0"; failures=$((failures + 1)); }

# Once 0.1.0 is stable, the base must move above it, and no further than 0.2.x.
tag v0.1.0
expect error experimental
expect 0.1.1-rc.1 rc 0.1.1
expect 0.2.0-experimental.1 experimental 0.2.0
expect 0.2.3-experimental.1 experimental 0.2.3
expect error experimental 0.3.0
expect 1.0.0-beta.1 beta 1.0.0

# From 1.0.0 on the cap is the next major.
tag v1.0.0
tag v1.4.2
expect 1.5.0-experimental.1 experimental 1.5.0
expect 2.0.0-experimental.1 experimental 2.0.0
expect 2.7.1-experimental.1 experimental 2.7.1
expect error experimental 3.0.0
expect error experimental 1.4.2

if ((failures)); then
    echo "$failures failure(s)"
    exit 1
fi
echo "version.sh: all checks passed"
