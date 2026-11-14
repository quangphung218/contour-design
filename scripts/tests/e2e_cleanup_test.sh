#!/usr/bin/env bash
# crates/od-ui/e2e/run.sh leaves no daemon behind: a scenario that dies
# before it stops the daemon it spawned (an uncaught error), and run.sh
# itself stopped mid-scenario (SIGTERM), both end that daemon. Stand-in
# scenarios spawn `sleep` as their daemon; nothing is built
# (OD_E2E_SKIP_BUILD=1).
# Usage: scripts/tests/e2e_cleanup_test.sh   (or `make test-scripts`)
set -uo pipefail
cd "$(dirname "$0")/../.."
run="$PWD/crates/od-ui/e2e/run.sh"
failures=0
fail() { echo "FAIL $1"; failures=$((failures + 1)); }
command -v node > /dev/null || { echo "FAIL node is not on PATH: the e2e runner needs it" >&2; exit 1; }

tmp="$(mktemp -d)"
cleanup() {
  # Whatever a failing case left running.
  for f in "$tmp"/*.pid; do [ -f "$f" ] && kill "$(cat "$f")" 2> /dev/null; done
  rm -rf "$tmp"
}
trap cleanup EXIT

# scenario NAME TAIL: a scenario that spawns its daemon (`sleep 300`, its
# pid in NAME.pid) as the e2e scripts do, then runs TAIL.
scenario() {
  cat > "$tmp/$1.mjs" << EOF
import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
const daemon = spawn('sleep', ['300'], { stdio: 'ignore' });
writeFileSync('$tmp/$1.pid', String(daemon.pid));
$2
EOF
}

# gone PIDFILE: the process in PIDFILE ends within 5 s.
gone() {
  local pid i
  pid="$(cat "$1" 2> /dev/null)" || return 1
  for i in $(seq 1 50); do
    kill -0 "$pid" 2> /dev/null || return 0
    sleep 0.1
  done
  return 1
}

# 1. The scenario throws without stopping its daemon.
scenario crash "throw new Error('scenario failed');"
OD_E2E_SKIP_BUILD=1 "$run" "$tmp/crash.mjs" > "$tmp/crash.log" 2>&1
rc=$?
[ "$rc" != 0 ] || fail "run.sh passed a scenario that threw"
[ -f "$tmp/crash.pid" ] || fail "the crashing scenario never started its daemon: $(cat "$tmp/crash.log")"
gone "$tmp/crash.pid" || fail "a scenario that threw left its daemon running"

# 2. run.sh is stopped while the scenario runs.
scenario hang "setInterval(() => {}, 1000);"
OD_E2E_SKIP_BUILD=1 "$run" "$tmp/hang.mjs" > "$tmp/hang.log" 2>&1 &
runner=$!
for i in $(seq 1 100); do [ -f "$tmp/hang.pid" ] && break; sleep 0.1; done
if [ -f "$tmp/hang.pid" ]; then
  kill -TERM "$runner"
  wait "$runner"
  rc=$?
  [ "$rc" != 0 ] || fail "run.sh stopped by SIGTERM exited 0"
  gone "$tmp/hang.pid" || fail "run.sh stopped by SIGTERM left the scenario's daemon running"
else
  kill "$runner" 2> /dev/null
  fail "the hanging scenario never started its daemon: $(cat "$tmp/hang.log")"
fi

# 3. A scenario that passes still passes.
cat > "$tmp/pass.mjs" << 'EOF'
console.log('PASS');
EOF
OD_E2E_SKIP_BUILD=1 "$run" "$tmp/pass.mjs" > "$tmp/pass.log" 2>&1 || fail "a passing scenario failed: $(cat "$tmp/pass.log")"

[ "$failures" -eq 0 ] || { echo "e2e cleanup: $failures check(s) failed" >&2; exit 1; }
echo "e2e cleanup: all checks passed"
