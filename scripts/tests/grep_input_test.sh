#!/usr/bin/env bash
# The grep-based checks must fail when they cannot search, not pass:
# outside a git repository (a `git archive` export) `git grep` exits 128,
# which is an error, while exit 1 only means "no match".
# Usage: scripts/tests/grep_input_test.sh   (or `make test-scripts`)
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
failures=0
for t in wording_test.sh chrome_sandbox_test.sh; do
  work="$(mktemp -d)"
  mkdir -p "$work/scripts/tests"
  cp "$here/$t" "$work/scripts/tests/"
  (cd "$work" && GIT_CEILING_DIRECTORIES="$work" scripts/tests/"$t") > "$work/out" 2>&1
  rc=$?
  if [ "$rc" -ne 0 ] && grep -q "could not search" "$work/out"; then
    echo "ok   $t fails outside a git repository"
  else
    echo "FAIL $t passed (rc $rc) without anything to search:"
    sed 's/^/     /' "$work/out"
    failures=$((failures + 1))
  fi
  rm -rf "$work"
done
[ "$failures" -eq 0 ] && echo "grep inputs: all checks passed" || { echo "grep inputs: $failures failed"; exit 1; }
