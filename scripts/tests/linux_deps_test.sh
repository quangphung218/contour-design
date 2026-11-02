#!/usr/bin/env bash
# Tests for scripts/linux-deps.sh with fake `sudo`, `timeout` and `apt-get`
# on PATH: a failed apt-get call is tried again, three failures stop the
# script, and every call carries the network and lock timeouts.
# Usage: scripts/tests/linux_deps_test.sh   (or `make test-scripts`)
# The check strings are eval-ed later, so single quotes are meant.
# shellcheck disable=SC2016,SC2034
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../linux-deps.sh"
failures=0

setup() {
  work="$(mktemp -d)"
  mkdir -p "$work/bin"
  printf '#!/usr/bin/env bash\nexec "$@"\n' > "$work/bin/sudo"
  # The fake timeout records the limit it was given, then runs the command.
  printf '#!/usr/bin/env bash\necho "limit $1" >> "%s/calls"\nshift\nexec "$@"\n' "$work" > "$work/bin/timeout"
  # The fake apt-get fails the first FAIL_UPDATES `update` calls.
  cat > "$work/bin/apt-get" <<SH
#!/usr/bin/env bash
echo "apt-get \$*" >> "$work/calls"
if [[ " \$* " == *" update "* ]]; then
  n=\$(grep -c ' update' "$work/calls")
  [ "\$n" -gt "\${FAIL_UPDATES:-0}" ] || exit 100
fi
SH
  chmod +x "$work/bin/"*
}

check() {
  if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; failures=$((failures + 1)); fi
}

setup
PATH="$work/bin:$PATH" APT_STEP_SECONDS=7 APT_RETRY_SECONDS=0 FAIL_UPDATES=1 \
  "$script" > "$work/out" 2>&1
rc=$?
check "one failed update is tried again and the install runs" '[ $rc -eq 0 ] && [ "$(grep -c "apt-get .* update" "$work/calls")" -eq 2 ] && grep -q "apt-get .* install" "$work/calls"'
check "the failure is reported" 'grep -q "try 1 of 3" "$work/out"'
check "every call has the step limit" '[ "$(grep -c "^limit 7$" "$work/calls")" -eq 3 ]'
check "every call has network and lock timeouts" '! grep "^apt-get" "$work/calls" | grep -v "Acquire::http::Timeout=30" | grep -q . && ! grep "^apt-get" "$work/calls" | grep -v "DPkg::Lock::Timeout=120" | grep -q .'
rm -rf "$work"

setup
PATH="$work/bin:$PATH" APT_RETRY_SECONDS=0 FAIL_UPDATES=9 "$script" > "$work/out" 2>&1
rc=$?
check "three failed tries stop the script" '[ $rc -ne 0 ] && [ "$(grep -c "apt-get .* update" "$work/calls")" -eq 3 ] && ! grep -q "apt-get .* install" "$work/calls"'
rm -rf "$work"

[ "$failures" -eq 0 ] || { echo "$failures failed"; exit 1; }
