#!/usr/bin/env bash
# Product-name wording that a blind rename gets wrong ("an OpenDesign" became
# "an Contour"). Checks every tracked file except vendored content.
# Usage: scripts/tests/wording_test.sh   (or `make test-scripts`)
set -uo pipefail
cd "$(dirname "$0")/../.."
hits="$(git grep -n -i -w 'an Contour' -- . ':!content' ':!scripts/tests/wording_test.sh' || true)"
if [ -n "$hits" ]; then
  echo "FAIL \"an Contour\" should read \"a Contour\":"
  printf '%s\n' "$hits" | sed 's/^/     /'
  exit 1
fi
echo "wording: all checks passed"
