#!/usr/bin/env bash
# Tests for scripts/ci-retry.sh with a stand-in command that fails a set
# number of times before it passes.
# Usage: scripts/tests/ci_retry_test.sh   (or `make test-scripts`)
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../ci-retry.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0
fail() { echo "FAIL $1"; failures=$((failures + 1)); }

# flaky N: fails its first N runs (exit 7), then passes; counts runs.
cat > "$work/flaky" <<'SH'
#!/bin/sh
n=$(($(cat "$COUNT" 2>/dev/null || echo 0) + 1))
echo "$n" > "$COUNT"
[ "$n" -gt "$1" ] || exit 7
SH
chmod +x "$work/flaky"
attempt() { rm -f "$work/count"; COUNT="$work/count" CI_RETRY_SECONDS=0 "$script" "$work/flaky" "$@" > "$work/out" 2>&1; }
runs() { cat "$work/count"; }

attempt 0; rc=$?
[ "$rc" = 0 ] && [ "$(runs)" = 1 ] || fail "a passing command runs once (rc $rc, runs $(runs))"
attempt 2; rc=$?
[ "$rc" = 0 ] && [ "$(runs)" = 3 ] || fail "two failures then a pass: passes on the third try (rc $rc, runs $(runs))"
grep -q '::warning::' "$work/out" || fail "a failed try prints a warning"
attempt 5; rc=$?
[ "$rc" = 7 ] && [ "$(runs)" = 3 ] || fail "stops after 3 tries with the command's exit code (rc $rc, runs $(runs))"
rm -f "$work/count"
COUNT="$work/count" CI_RETRY_SECONDS=0 CI_RETRY_TRIES=1 "$script" "$work/flaky" 1 > /dev/null 2>&1; rc=$?
[ "$rc" = 7 ] && [ "$(runs)" = 1 ] || fail "CI_RETRY_TRIES=1 tries once (rc $rc, runs $(runs))"
"$script" > /dev/null 2>&1; rc=$?
[ "$rc" = 2 ] || fail "no command: usage error, exit 2 (got $rc)"

[ "$failures" -eq 0 ] || { echo "ci-retry: $failures check(s) failed" >&2; exit 1; }
echo "ci-retry: all checks passed"
