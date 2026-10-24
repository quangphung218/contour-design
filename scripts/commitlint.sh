#!/usr/bin/env bash
# Checks commit messages in <base>..<head> against commitlint.config.mjs.
# Usage: scripts/commitlint.sh [base] [head]   (default: main..HEAD; or `make commitlint`)
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
base="${1:-main}"
head="${2:-HEAD}"
cd "$root"
if [ -z "$(git rev-list "$base..$head")" ]; then
  echo "commitlint: no commits in $base..$head"
  exit 0
fi
# Pinned; bump together with the CI job.
exec npx --yes @commitlint/cli@21.2.4 --config commitlint.config.mjs --from "$base" --to "$head" --verbose
