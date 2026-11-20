#!/usr/bin/env bash
# Tests for scripts/changelog-lint.sh, then runs it on the real CHANGELOG.md.
# Usage: scripts/tests/changelog_lint_test.sh   (or `make test-scripts`)
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
lint="$here/../changelog-lint.sh"
failures=0
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

expect() { # expect <0|1> <name> <file>
  "$lint" "$3" > "$work/out" 2>&1; rc=$?
  if [ "$rc" -ne "$1" ]; then
    echo "FAIL: $2 (rc $rc, want $1)"; cat "$work/out"; failures=$((failures + 1))
  else
    echo "ok: $2"
  fi
}

cat > "$work/good.md" <<'MD'
## [Unreleased]

### Fixed

- Exports fail within a minute instead of hanging when Chrome quits (#270).

<!-- changelog-lint: stop -->

## [0.1.0]

- An old entry that is long and wraps
  onto a second line, from before the rule, one two three four five six seven eight nine ten.
MD
expect 0 "short one-line entries pass; entries below the marker are not checked" "$work/good.md"

cat > "$work/long.md" <<'MD'
## [Unreleased]

- One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twenty-one twenty-two twenty-three twenty-four twenty-five twenty-six.
MD
expect 1 "an entry over 25 words fails" "$work/long.md"

cat > "$work/wrap.md" <<'MD'
## [Unreleased]

- A short entry
  that wraps.
MD
expect 1 "a wrapped entry fails" "$work/wrap.md"

cat > "$work/refs.md" <<'MD'
## [Unreleased]

- One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twenty-one twenty-two twenty-three twenty-four twenty-five (#1, #2).
MD
expect 0 "PR references are not counted" "$work/refs.md"

expect 0 "the repository CHANGELOG.md passes" "$here/../../CHANGELOG.md"

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
