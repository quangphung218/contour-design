#!/usr/bin/env bash
# Tests for scripts/changelog-placement.sh on a throwaway git repo.
# Usage: scripts/tests/changelog_placement_test.sh   (or `make test-scripts`)
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
check="$here/../changelog-placement.sh"
failures=0
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$work" && git init -q . && git config user.email t@t && git config user.name t
cat > CHANGELOG.md <<'MD'
# Changelog

## [Unreleased]

## [0.2.0] - 2026-11-20

### Fixed

- An old line (#1).
MD
git add . && git commit -qm base && base=$(git rev-parse HEAD)

expect() { # expect <0|1> <name>
  "$check" "$base" HEAD > out 2>&1; rc=$?
  if [ "$rc" -ne "$1" ]; then echo "FAIL: $2 (rc $rc, want $1)"; cat out; failures=$((failures + 1)); else echo "ok: $2"; fi
  git reset -q --hard "$base"
}

perl -0pi -e 's/## \[Unreleased\]\n/## [Unreleased]\n\n### Fixed\n\n- A new line (#2).\n/' CHANGELOG.md && git commit -qam new
expect 0 "a line under [Unreleased] passes"

perl -0pi -e 's/- An old line \(#1\)\.\n/- An old line (#1).\n- A new line in the released section (#2).\n/' CHANGELOG.md && git commit -qam misplaced
expect 1 "a line added to a released section fails"

perl -0pi -e 's/## \[Unreleased\]\n/## [Unreleased]\n\n## [0.2.1] - 2026-11-21\n/' CHANGELOG.md && git commit -qam release
expect 0 "a release (new heading, no new bullets) passes"

echo "x" > other && git add other && git commit -qm other
expect 0 "a branch that does not touch the CHANGELOG passes"

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
