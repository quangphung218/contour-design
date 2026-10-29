#!/usr/bin/env bash
# Builds the UI bundle and the `od` daemon, then runs the Playwright e2e
# against the real daemon with the mock `claude` CLI in e2e/mock-agent.
# Usage: crates/od-ui/e2e/run.sh [script.mjs…]   (from anywhere; needs trunk,
# node, npm). With arguments, runs only those scripts (e.g. demo.mjs).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
ui="$here/.."
root="$(cd "$ui/../.." && pwd)"

(cd "$ui" && trunk build)
cargo build -p od-cli --bin od --manifest-path "$root/Cargo.toml"
target_dir="$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).target_directory))')"

cd "$here"
[ -d node_modules/playwright ] && [ -d node_modules/axe-core ] || npm ci --no-audit --no-fund
export OD_UI_DIST="$ui/dist" OD_E2E_BIN="$target_dir/debug/od" OD_MOCK_AGENTS="$here/mock-agent" \
  OD_RESOURCE_ROOT="$root/content"
# A fixed port collides when two worktrees run the e2e at once, so each
# script gets a free port of its own, taken right before it starts, unless
# OD_E2E_PORT is given (then the scripts use it and the next twelve ports).
# The ports come from 20000-32767, below the ephemeral range on Linux
# (32768-60999) and macOS (49152-65535): a port from listen(0) lies in that
# range, and the browser's client sockets, open or in TIME_WAIT for a
# minute, took base + n before its script bound it ("Address already in
# use" on CI).
fixed_port="${OD_E2E_PORT:-}"
free_port() {
  node -e '
    const net = require("net");
    const pick = (left) => {
      const port = 20000 + Math.floor(Math.random() * 12768);
      const s = net.createServer().once("error", () => {
        if (left > 0) pick(left - 1); else process.exit(1);
      });
      s.listen(port, "127.0.0.1", () => { console.log(port); s.close(); });
    };
    pick(50);'
}
# run VAR script: runs the script with its port variable set to a free port.
run() {
  if [ -n "$fixed_port" ]; then
    node "$2"
  else
    env "$1=$(free_port)" node "$2"
  fi
}
if [ $# -gt 0 ]; then
  [ -n "$fixed_port" ] || { OD_E2E_PORT="$(free_port)"; export OD_E2E_PORT; }
  for script in "$@"; do node "$script"; done
  exit 0
fi
run OD_E2E_PORT studio.e2e.mjs
run OD_E2E_DS_PORT design-system.e2e.mjs
run OD_E2E_LIB_PORT library.e2e.mjs
run OD_E2E_MANAGE_PORT manage.e2e.mjs
run OD_E2E_CHAT_PORT chat.e2e.mjs
run OD_E2E_FILES_PORT files.e2e.mjs
run OD_E2E_PREVIEW_PORT preview.e2e.mjs
# Wave C tracks (with OD_E2E_PORT: + 5 + track number, C1 = +6 … C4 = +9).
run OD_E2E_C2_PORT c2.e2e.mjs
run OD_E2E_C3_PORT c3-runtimes.e2e.mjs
run OD_E2E_C4_PORT c4-depth.e2e.mjs
# axe-core and narrow windows (with OD_E2E_PORT: + 10).
run OD_E2E_A11Y_PORT a11y.e2e.mjs
# Check for Updates against a stub GitHub Releases (with OD_E2E_PORT: + 11).
run OD_E2E_UPDATES_PORT updates.e2e.mjs
# Settings ▸ About ▸ Your usage (with OD_E2E_PORT: + 12).
run OD_E2E_USAGE_PORT usage.e2e.mjs
