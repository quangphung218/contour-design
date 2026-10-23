#!/usr/bin/env bash
# Builds the UI bundle and the `od` daemon, then runs the Playwright e2e
# against the real daemon with the mock `claude` CLI in e2e/mock-agent.
# Usage: crates/od-ui/e2e/run.sh   (from anywhere; needs trunk, node, npm)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
ui="$here/.."
root="$(cd "$ui/../.." && pwd)"

(cd "$ui" && trunk build)
cargo build -p od-cli --bin od --manifest-path "$root/Cargo.toml"
target_dir="$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).target_directory))')"

cd "$here"
[ -d node_modules/playwright ] || npm ci --no-audit --no-fund
export OD_UI_DIST="$ui/dist" OD_E2E_BIN="$target_dir/debug/od" OD_MOCK_AGENTS="$here/mock-agent" \
  OD_RESOURCE_ROOT="$root/content"
# A fixed port collides when two worktrees run the e2e at once; pick a free
# one (the other scenarios use the next five ports) unless one is given.
if [ -z "${OD_E2E_PORT:-}" ]; then
  OD_E2E_PORT="$(node -e 'const s=require("net").createServer().listen(0,"127.0.0.1",()=>{console.log(s.address().port);s.close()})')"
  export OD_E2E_PORT
fi
node studio.e2e.mjs
node design-system.e2e.mjs
node library.e2e.mjs
node manage.e2e.mjs
node chat.e2e.mjs
node files.e2e.mjs
# Wave C tracks: port OD_E2E_PORT + track number (C2 = +7).
node c2.e2e.mjs
