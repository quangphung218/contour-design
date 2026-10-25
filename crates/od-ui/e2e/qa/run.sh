#!/usr/bin/env bash
# QA suite (docs/qa/test-plan.md): business-flow e2e against the real `od`
# daemon and the mock agents, one file per flow group. Not part of CI or
# `make e2e`. Prints pass / known bug (QA-NNN) / unexpected failure.
# Usage: crates/od-ui/e2e/qa/run.sh [file-filter]   e.g. run.sh projects
#   QA_SKIP_BUILD=1  reuse the built UI bundle and `od`
#   QA_ONLY=<regex>  run only tests whose "ID name" matches
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
e2e="$here/.."
ui="$e2e/.."
root="$(cd "$ui/../.." && pwd)"
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}" CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/odrs-target-$(basename "$root")}"

if [ -z "${QA_SKIP_BUILD:-}" ]; then
  (cd "$ui" && trunk build) || exit 1
  cargo build -p od-cli --bin od --manifest-path "$root/Cargo.toml" || exit 1
fi

cd "$e2e"
[ -d node_modules/playwright ] || npm ci --no-audit --no-fund || exit 1
export OD_UI_DIST="$ui/dist" OD_E2E_BIN="$CARGO_TARGET_DIR/debug/od" OD_MOCK_AGENTS="$e2e/mock-agent" \
  OD_RESOURCE_ROOT="$root/content"
[ -x "$OD_E2E_BIN" ] || { echo "missing $OD_E2E_BIN (build first)" >&2; exit 1; }

rm -rf "$e2e/test-results/qa"
for f in "$here"/*.qa.mjs; do
  name="$(basename "$f" .qa.mjs)"
  [ -n "${1:-}" ] && [[ "$name" != *"$1"* ]] && continue
  echo "== $name"
  node "$f"
done
node "$here/summary.mjs" "${1:-}"
