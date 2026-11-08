#!/usr/bin/env bash
# Builds the UI bundle and the `od` daemon, then runs the Playwright e2e
# against the real daemon with the mock `claude` CLI in e2e/mock-agent.
# Usage: crates/od-ui/e2e/run.sh [script.mjs…]   (from anywhere; needs trunk,
# node, npm). With arguments, runs only those scripts, one after another
# (e.g. demo.mjs). Without, runs every scenario, OD_E2E_JOBS at a time
# (default 3 on CI, 2 elsewhere: each scenario has its own browser and
# daemon, and the debug UI bundle takes about 1 GB per tab).
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
# OD_E2E_PORT is given (then the scripts use it and the next seventeen
# ports). The ports come from 20000-32767, below the ephemeral range on
# Linux (32768-60999) and macOS (49152-65535): a port from listen(0) lies in
# that range, and the browser's client sockets, open or in TIME_WAIT for a
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
if [ $# -gt 0 ]; then
  [ -n "$fixed_port" ] || { OD_E2E_PORT="$(free_port)"; export OD_E2E_PORT; }
  for script in "$@"; do node "$script"; done
  exit 0
fi

# Every scenario, as "PORT_VARIABLE script", the longest first so the last
# ones to start are short. With OD_E2E_PORT, a script's port is
# OD_E2E_PORT + its offset (in the script). The preview bridge needs no
# daemon and no port ("-").
specs=(
  "OD_E2E_C4_PORT c4-depth.e2e.mjs"          # + 9; 19 languages × 12 screens
  "OD_E2E_FILES_PORT files.e2e.mjs"          # + 5
  "OD_E2E_CHAT_PORT chat.e2e.mjs"            # + 4
  "OD_E2E_MANAGE_PORT manage.e2e.mjs"        # + 3
  "OD_E2E_LIB_PORT library.e2e.mjs"          # + 2
  "OD_E2E_PORT studio.e2e.mjs"               # + 0
  "OD_E2E_PREVIEW_PORT preview.e2e.mjs"      # + 6 (track C1)
  "OD_E2E_A11Y_PORT a11y.e2e.mjs"            # + 10; axe-core, narrow windows
  "OD_E2E_UPDATES_PORT updates.e2e.mjs"      # + 11; stub GitHub Releases
  "OD_E2E_C3_PORT c3-runtimes.e2e.mjs"       # + 8
  "OD_E2E_C2_PORT c2.e2e.mjs"                # + 7
  "OD_E2E_DS_PORT design-system.e2e.mjs"     # + 1
  "OD_E2E_MOTION_PORT motion.e2e.mjs"        # + 15; MP4 through `od media`
  "- bridge.e2e.mjs"                         # Chromium and WebKit, no daemon
  "OD_E2E_REVIEW_PORT review.e2e.mjs"        # + 17; 2026-11-04 design review
  "OD_E2E_POLISH_PORT polish.e2e.mjs"        # + 14; beta.7 design review
  "OD_E2E_USAGE_PORT usage.e2e.mjs"          # + 12; Settings ▸ About ▸ usage
  "OD_E2E_SETUP_PORT setup.e2e.mjs"          # + 13; first-run setup
  "OD_E2E_GALLERY_PORT gallery.e2e.mjs"      # + 16; a remixed gallery deck
)

jobs_max="${OD_E2E_JOBS:-$([ -n "${CI:-}" ] && echo 3 || echo 2)}"
[[ "$jobs_max" =~ ^[1-9][0-9]*$ ]] || { echo "OD_E2E_JOBS must be a positive number, not '$jobs_max'" >&2; exit 2; }
# One log per scenario, printed whole when it ends, so lines of scenarios
# running together do not interleave.
logs="$here/test-results/logs"
rm -rf "$logs"
mkdir -p "$logs"

# Running scenarios, as parallel arrays (bash 3.2 on macOS has neither
# associative arrays nor `wait -n`).
pids=() names=() starts=()
used_ports=" "
failed=()
started_all=$(date +%s)
cleanup() {
  for pid in ${pids[@]+"${pids[@]}"}; do kill "$pid" 2>/dev/null || true; done
}
trap cleanup EXIT
trap 'exit 130' INT TERM

start() {
  local var="$1" script="$2" port
  local name="${script%.mjs}"
  if [ "$var" = - ] || [ -n "$fixed_port" ]; then
    node "$script" >"$logs/$name.log" 2>&1 &
  else
    # Two scenarios starting together must not draw the same port.
    port="$(free_port)"
    while [[ "$used_ports" == *" $port "* ]]; do port="$(free_port)"; done
    used_ports="$used_ports$port "
    env "$var=$port" node "$script" >"$logs/$name.log" 2>&1 &
  fi
  pids+=($!) names+=("$name") starts+=("$(date +%s)")
}

# Waits until one scenario ends, prints its log and drops it from the arrays.
reap() {
  local i rc
  while :; do
    for i in "${!pids[@]}"; do
      kill -0 "${pids[$i]}" 2>/dev/null && continue
      rc=0
      wait "${pids[$i]}" || rc=$?
      local name="${names[$i]}" took=$(($(date +%s) - starts[i]))
      echo "--- ${name} ($([ "$rc" = 0 ] && echo PASS || echo "FAIL, exit $rc"), ${took}s)"
      cat "$logs/$name.log"
      [ "$rc" = 0 ] || failed+=("$name")
      unset 'pids[i]' 'names[i]' 'starts[i]'
      pids=(${pids[@]+"${pids[@]}"}) names=(${names[@]+"${names[@]}"}) starts=(${starts[@]+"${starts[@]}"})
      return
    done
    sleep 1
  done
}

echo "e2e: ${#specs[@]} scenarios, $jobs_max at a time; logs in $logs"
for spec in "${specs[@]}"; do
  while [ "${#pids[@]}" -ge "$jobs_max" ]; do reap; done
  start "${spec%% *}" "${spec#* }"
done
while [ "${#pids[@]}" -gt 0 ]; do reap; done

echo "--- e2e: $(( ${#specs[@]} - ${#failed[@]} ))/${#specs[@]} passed in $(($(date +%s) - started_all))s"
if [ "${#failed[@]}" -gt 0 ]; then
  # The failing logs again, last, where CI shows them.
  for name in "${failed[@]}"; do
    echo "--- FAIL ${name}: $logs/$name.log"
    cat "$logs/$name.log"
  done
  echo "e2e failed: ${failed[*]}" >&2
  exit 1
fi
