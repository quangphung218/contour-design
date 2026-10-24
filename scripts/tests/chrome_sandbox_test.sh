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

calls="$(git grep -n -E 'chrome_sandbox_works\(|without_chrome_sandbox\(' -- 'crates/*/src/**' 'apps/**' \
  | grep -v -E ':\s*(///|//)' | grep -v -E 'pub fn (chrome_sandbox_works|without_chrome_sandbox)\(' || true)"
[ -z "$calls" ] || fail "product code calls the sandbox probe or opts out of the sandbox" "$calls"

sites="$(git grep -n -F '.no_sandbox()' -- 'crates/*/src/**' 'apps/**' || true)"
[ "$(printf '%s' "$sites" | grep -c .)" -le 1 ] && printf '%s' "$sites" | grep -q 'od-render-cdp/src/lib.rs' \
  || fail "--no-sandbox is set somewhere else than the opt-in in od-render-cdp" "$sites"
guard="$(grep -n -B1 -F 'config = config.no_sandbox();' crates/od-render-cdp/src/lib.rs | head -1)"
printf '%s' "$guard" | grep -q 'if no_sandbox {' || fail "the --no-sandbox site is not behind the opt-in flag" "$guard"

[ "$failures" -eq 0 ] && echo "chrome sandbox: all checks passed" || { echo "chrome sandbox: $failures failed"; exit 1; }
