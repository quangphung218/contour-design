#!/usr/bin/env bash
# Product-name wording that a blind rename gets wrong ("an OpenDesign" became
# "an Contour"). Checks every tracked file except vendored content.
# Usage: scripts/tests/wording_test.sh   (or `make test-scripts`)
set -uo pipefail
cd "$(dirname "$0")/../.."
# git grep: 0 = matches, 1 = none, anything else = could not search (FAIL).
hits="$(git grep -n -i -w 'an Contour' -- . ':!content' ':!scripts/tests/wording_test.sh')"
rc=$?
if [ "$rc" -gt 1 ]; then
  echo "FAIL git grep could not search (rc $rc): run inside the repository"
  exit 1
fi
if [ -n "$hits" ]; then
  echo "FAIL \"an Contour\" should read \"a Contour\":"
  printf '%s\n' "$hits" | sed 's/^/     /'
  exit 1
fi
echo "wording: all checks passed"
