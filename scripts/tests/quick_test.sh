#!/usr/bin/env bash
# Tests for the crate selection of scripts/quick.sh (`make quick`) against
# this workspace's real crate graph (cargo metadata, nothing is built).
# Usage: scripts/tests/quick_test.sh   (or `make test-scripts`)
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../quick.sh"
failures=0
# expect NAME WANT FILE…: the selected crates, space-separated.
expect() {
  local name="$1" want="$2" got
  shift 2
  got="$("$script" --packages "$@" | tr '\n' ' ' | sed 's/ $//')"
  if [ "$got" = "$want" ]; then echo "ok   $name"; else echo "FAIL $name"; echo "     want: $want"; echo "     got:  $got"; failures=$((failures + 1)); fi
}

expect "a crate and its direct dependents" "od-job od-mcp-client od-runtime od-term" crates/od-job/src/lib.rs
expect "only direct dependents, not theirs" "od-cli od-content od-contracts od-live od-preview od-runs od-runtime od-server od-store od-ui" crates/od-contracts/src/lib.rs
expect "a crate nothing depends on" "od-ui" crates/od-ui/src/app.rs
expect "the desktop app" "od-desktop" apps/desktop/src/main.rs
expect "two crates" "od-desktop od-media od-server" crates/od-media/src/lib.rs apps/desktop/build.rs
expect "docs select nothing" "" docs/development.md README.md
expect "content selects the crates whose tests read it" "od-cli od-content od-preview od-server" content/skills/a/SKILL.md
expect "the preview bridge selects the crates that embed it" "od-preview od-render-cdp" bridges/od-bridge.js
expect "Cargo.lock selects every crate" "*" Cargo.lock crates/od-ui/src/app.rs
expect "a workspace Cargo.toml selects every crate" "*" Cargo.toml
expect "a crate's own Cargo.toml selects that crate" "od-cli od-export od-render-cdp od-server" crates/od-export/Cargo.toml
expect "a prefix of a crate name is not that crate" "" crates/od-ui-extra/x.rs

[ "$failures" -eq 0 ] || { echo "quick: $failures check(s) failed" >&2; exit 1; }
echo "quick: all checks passed"
