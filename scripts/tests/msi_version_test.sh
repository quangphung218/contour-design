#!/usr/bin/env bash
# Tests for scripts/msi-version.sh: every release version maps to a numeric
# MSI version that keeps release order, and versions WiX cannot hold fail.
# Usage: scripts/tests/msi_version_test.sh   (or `make test-scripts`)
set -uo pipefail
script="$(cd "$(dirname "$0")/.." && pwd)/msi-version.sh"
failures=0

expect() {
  local got
  got="$("$script" "$1" 2>/dev/null)"
  if [ "$got" = "$2" ]; then echo "ok   $1 -> $2"; else echo "FAIL $1: want $2, got '$got'"; failures=$((failures + 1)); fi
}
refuse() {
  if "$script" "$1" > /dev/null 2>&1; then echo "FAIL $1 should be refused"; failures=$((failures + 1)); else echo "ok   $1 refused"; fi
}

expect 0.2.0-alpha.3 0.2.0.3
expect 0.2.0-beta.2 0.2.0.102
expect v0.2.0-beta.2 0.2.0.102
expect 0.2.0-rc.1 0.2.0.201
expect 0.2.0 0.2.0.1000
expect 1.10.65535 1.10.65535.1000
refuse 0.2.0-beta
refuse 0.2.0-preview.1
refuse 0.2.0-beta.100
refuse 256.0.0
refuse 0.2.65536
refuse 0.2

order="$(for v in 0.2.0 0.2.0-rc.1 0.2.0-alpha.3 0.2.0-beta.9 0.2.0-beta.10; do "$script" "$v"; done | sort -t. -k4,4n | tr '\n' ' ')"
if [ "$order" = "0.2.0.3 0.2.0.109 0.2.0.110 0.2.0.201 0.2.0.1000 " ]; then echo "ok   alpha < beta < rc < release"; else echo "FAIL order: $order"; failures=$((failures + 1)); fi

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
