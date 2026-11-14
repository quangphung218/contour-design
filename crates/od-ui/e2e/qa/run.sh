#!/usr/bin/env bash
# QA suite (docs/qa/test-plan.md): business-flow e2e against the real `od`
# daemon and the mock agents, one file per flow group. Not part of CI or
# `make e2e`. Prints pass / known bug (QA-NNN) / unexpected failure.
# Usage: crates/od-ui/e2e/qa/run.sh [file-filter]   e.g. run.sh projects
#   QA_SKIP_BUILD=1  reuse the built UI bundle and `od`
#   QA_ONLY=<regex>  run only tests whose "ID name" matches
# A flow file that dies before it stops its daemon (an uncaught error, run.sh
# itself stopped) leaves no `od daemon start` behind: each file runs in a
# process group of its own, ended when the file ends and on exit (as
# e2e/run.sh does).
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

# Job control: the background node gets a process group of its own, whose
# id is its pid, so `kill -- -PID` ends it and the daemon it spawned, even
# once node is gone. Ctrl+C goes to run.sh, which ends that group.
set -m
pid=
# end_group PID: ends what is left of the flow file PID started.
end_group() { kill -TERM -- "-$1" 2>/dev/null || true; }
cleanup() { [ -z "$pid" ] || end_group "$pid"; }
trap cleanup EXIT
trap 'exit 130' INT TERM

rm -rf "$e2e/test-results/qa"
for f in "$here"/*.qa.mjs; do
  name="$(basename "$f" .qa.mjs)"
  [ -n "${1:-}" ] && [[ "$name" != *"$1"* ]] && continue
  echo "== $name"
  # Its exit code is not the verdict: summary.mjs reads the results.
  node "$f" &
  pid=$!
  wait "$pid"
  end_group "$pid"
  pid=
done
node "$here/summary.mjs" "${1:-}"
