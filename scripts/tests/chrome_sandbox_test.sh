#!/usr/bin/env bash
# Product code never turns Chrome's sandbox off by itself: the only way is
# the OD_CHROME_NO_SANDBOX=1 opt-in. The sandbox probe and the builder that
# skips the sandbox are for tests; outside crates/*/tests nothing may call
# them, and `.no_sandbox()` appears once, behind the opt-in flag.
# Usage: scripts/tests/chrome_sandbox_test.sh   (or `make test-scripts`)
set -uo pipefail
cd "$(dirname "$0")/../.."
failures=0
fail() { echo "FAIL $1"; printf '%s\n' "$2" | sed 's/^/     /'; failures=$((failures + 1)); }

# search <git grep args…>: matches on stdout; git grep's 1 means none, any
# other failure (not a repository…) stops the test.
search() {
  local out rc
  out="$(git grep "$@")"
  rc=$?
  if [ "$rc" -gt 1 ]; then
    echo "FAIL git grep could not search (rc $rc): run inside the repository" >&2
    exit 1
  fi
  printf '%s' "$out"
}

calls="$(search -n -E 'chrome_sandbox_works\(|without_chrome_sandbox\(' -- 'crates/*/src/**' 'apps/**')" || exit 1
calls="$(printf '%s' "$calls" | grep -v -E ':\s*(///|//)' | grep -v -E 'pub fn (chrome_sandbox_works|without_chrome_sandbox)\(')"
[ -z "$calls" ] || fail "product code calls the sandbox probe or opts out of the sandbox" "$calls"

sites="$(search -n -F '.no_sandbox()' -- 'crates/*/src/**' 'apps/**')" || exit 1
[ "$(printf '%s' "$sites" | grep -c .)" -le 1 ] && printf '%s' "$sites" | grep -q 'od-render-cdp/src/lib.rs' \
  || fail "--no-sandbox is set somewhere else than the opt-in in od-render-cdp" "$sites"
# Nothing outside od-render-cdp may set the opt-in either (a server-side
# set_var or a `.env(...)` on the daemon would turn the sandbox off for every
# export while the checks above still pass).
opt_in="$(search -n -E 'OD_CHROME_NO_SANDBOX|NO_SANDBOX_ENV' -- 'crates/*/src/**' 'apps/**')" || exit 1
opt_in="$(printf '%s' "$opt_in" | grep -v '^crates/od-render-cdp/src/lib.rs:')"
[ -z "$opt_in" ] || fail "product code outside od-render-cdp names the --no-sandbox opt-in" "$opt_in"

guard="$(grep -n -B1 -F 'config = config.no_sandbox();' crates/od-render-cdp/src/lib.rs | head -1)"
printf '%s' "$guard" | grep -q 'if no_sandbox {' || fail "the --no-sandbox site is not behind the opt-in flag" "$guard"

[ "$failures" -eq 0 ] && echo "chrome sandbox: all checks passed" || { echo "chrome sandbox: $failures failed"; exit 1; }
