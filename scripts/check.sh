#!/usr/bin/env bash
# Parse every def file with the built parser and run the reference checker.
cd "$(dirname "$0")/.."
ELFIE="${ELFIE:-$HOME/elfie/target/debug/elfie}"
fail=0
for f in $(find def -name '*.lfy' | sort); do
  if ! "$ELFIE" --tree "$f" >/dev/null 2>"/tmp/elfie-check.err"; then
    echo "PARSE FAIL $f"; head -3 /tmp/elfie-check.err; fail=1
  fi
done
python3 scripts/refcheck.py || fail=1
exit $fail
