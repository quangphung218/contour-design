#!/usr/bin/env bash
# Builds the UI bundle and the mock daemon, then runs the Playwright e2e.
# Usage: crates/od-ui/e2e/run.sh   (from anywhere; needs trunk, node, npm)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
ui="$here/.."
root="$(cd "$ui/../.." && pwd)"

(cd "$ui" && trunk build)
cargo build -p od-ui --example mock_daemon --manifest-path "$root/Cargo.toml"
target_dir="$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).target_directory))')"

cd "$here"
[ -d node_modules/playwright ] || npm ci --no-audit --no-fund
OD_UI_DIST="$ui/dist" OD_MOCK_DAEMON="$target_dir/debug/examples/mock_daemon" node studio.e2e.mjs
