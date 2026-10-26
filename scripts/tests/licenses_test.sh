#!/usr/bin/env bash
# scripts/licenses.py check: passes on the committed files and fails when
# THIRD-PARTY-LICENSES no longer matches Cargo.lock.
# Usage: scripts/tests/licenses_test.sh   (or `make test-scripts`)
set -uo pipefail
cd "$(dirname "$0")/../.."
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fail=0

if ! out="$(scripts/licenses.py check 2>&1)"; then
  echo "FAIL check on the committed files: $out"
  fail=1
fi

# A crate that left the lock file but is still listed.
sed 's/^\(serde [0-9][^ ]*\) --/\1-old --/' THIRD-PARTY-LICENSES >"$tmp/third-party"
if cmp -s THIRD-PARTY-LICENSES "$tmp/third-party"; then
  echo "FAIL test setup: no serde line to change"
  fail=1
elif scripts/licenses.py check --third-party "$tmp/third-party" >/dev/null 2>&1; then
  echo "FAIL check passed with a stale crate list"
  fail=1
fi

[ "$fail" -eq 0 ] && echo "licenses: all checks passed"
exit "$fail"
