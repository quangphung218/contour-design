#!/usr/bin/env bash
# Tests for scripts/linux-deps.sh with fake `sudo`, `timeout` and `apt-get`
# on PATH: a failed apt-get call is tried again, three failures stop the
# script, every call carries the network and lock timeouts, the budget
# stops it with a clear error, and --playwright installs what a fake
# `playwright install-deps --dry-run` lists.
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
PATH="$work/bin:$PATH" APT_RETRY_SECONDS=0 "$script" ffmpeg > "$work/out" 2>&1
rc=$?
check "an extra package goes into the one install, under the step limit" '[ $rc -eq 0 ] && grep "apt-get .* install" "$work/calls" | grep -q " libssl-dev .* ffmpeg$" && [ "$(grep -c "apt-get .* install" "$work/calls")" -eq 1 ]'
rm -rf "$work"

setup
cat > "$work/bin/chown" <<SH
#!/usr/bin/env bash
echo "chown \$*" >> "$work/calls"
SH
chmod +x "$work/bin/chown"
PATH="$work/bin:$PATH" APT_RETRY_SECONDS=0 APT_ARCHIVES="$work/debs" GITHUB_OUTPUT="$work/output" "$script" > "$work/out" 2>&1
rc=$?
check "APT_ARCHIVES: every call keeps its .deb files there, autoclean, owned by the user after" '[ $rc -eq 0 ] && ! grep "^apt-get" "$work/calls" | grep -vq "Dir::Cache::Archives=$work/debs" && grep -q "^chown -R $(id -u):$(id -g) $work/debs$" "$work/calls" && grep -q "^apt-get .* autoclean$" "$work/calls"'
check "APT_ARCHIVES: nothing downloaded, archives-changed=false" '[ "$(cat "$work/output")" = archives-changed=false ]'
rm -rf "$work"

# A download cut by the limit: the fake apt-get writes a .deb, then fails
# every install; the folder is handed back and reported changed anyway.
setup
cat > "$work/bin/apt-get" <<SH
#!/usr/bin/env bash
echo "apt-get \$*" >> "$work/calls"
if [[ " \$* " == *" install "* ]]; then
  echo half > "$work/debs/partial/big.deb"
  exit 100
fi
SH
cat > "$work/bin/chown" <<SH
#!/usr/bin/env bash
echo "chown \$*" >> "$work/calls"
SH
chmod +x "$work/bin/"*
PATH="$work/bin:$PATH" APT_RETRY_SECONDS=0 APT_ARCHIVES="$work/debs" GITHUB_OUTPUT="$work/output" "$script" > "$work/out" 2>&1
rc=$?
check "APT_ARCHIVES: a failed install still hands the folder back, changed" '[ $rc -ne 0 ] && grep -q "^chown -R" "$work/calls" && [ "$(cat "$work/output")" = archives-changed=true ] && [ -f "$work/debs/partial/big.deb" ]'
rm -rf "$work"

setup
PATH="$work/bin:$PATH" APT_RETRY_SECONDS=0 "$script" > "$work/out" 2>&1
check "without APT_ARCHIVES apt keeps its own cache" '! grep -q "Dir::Cache::Archives" "$work/calls"'
rm -rf "$work"

setup
PATH="$work/bin:$PATH" APT_RETRY_SECONDS=0 FAIL_UPDATES=9 "$script" > "$work/out" 2>&1
rc=$?
check "three failed tries stop the script" '[ $rc -ne 0 ] && [ "$(grep -c "apt-get .* update" "$work/calls")" -eq 3 ] && ! grep -q "apt-get .* install" "$work/calls"'
rm -rf "$work"

# APT_BUDGET_SECONDS: each update takes a second and fails; the tries get
# what is left of 2 s, then the script stops with an error naming the
# mirror (on 19/11 a 47 kB/s mirror took 41 of the e2e job's 45 minutes).
setup
cat > "$work/bin/apt-get" <<SH
#!/usr/bin/env bash
echo "apt-get \$*" >> "$work/calls"
sleep 1
exit 100
SH
chmod +x "$work/bin/apt-get"
PATH="$work/bin:$PATH" APT_RETRY_SECONDS=0 APT_BUDGET_SECONDS=2 "$script" > "$work/out" 2>&1
rc=$?
check "APT_BUDGET_SECONDS: the first try gets the budget, not the step limit" '[ "$(sed -n "s/^limit //p" "$work/calls" | head -n1)" = 2 ]'
check "APT_BUDGET_SECONDS: no try gets more than what is left" '! sed -n "s/^limit //p" "$work/calls" | awk "\$1 > 2 || \$1 < 1" | grep -q .'
check "APT_BUDGET_SECONDS: spent, the script stops with an error about the mirror" '[ $rc -ne 0 ] && grep -q "^::error::linux-deps: apt-get update did not finish within APT_BUDGET_SECONDS=2: the Ubuntu package mirror is slow" "$work/out" && ! grep -q "apt-get .* install" "$work/calls"'
rm -rf "$work"

# --playwright DIR: a fake playwright that lists two missing packages on
# its first dry run and none after.
setup_playwright() {
  setup
  mkdir -p "$work/pw/node_modules/.bin"
  cat > "$work/pw/node_modules/.bin/playwright" <<SH
#!/usr/bin/env bash
echo "playwright \$* in \$PWD" >> "$work/calls"
n=\$(grep -c "^playwright " "$work/calls")
if [ "\$n" -le "\${PW_MISSING_RUNS:-1}" ]; then
  printf 'Missing system dependencies (2):\n  libfoo1\n  libbar-2.0\n'
  exit 1
fi
echo "All system dependencies are installed."
SH
  chmod +x "$work/pw/node_modules/.bin/playwright"
}

setup_playwright
PATH="$work/bin:$PATH" APT_RETRY_SECONDS=0 "$script" --playwright "$work/pw" ffmpeg > "$work/out" 2>&1
rc=$?
check "--playwright: what the dry run lists and the extras go into one install, no Tauri packages" '[ $rc -eq 0 ] && [ "$(grep -c "apt-get .* install" "$work/calls")" -eq 1 ] && grep "apt-get .* install" "$work/calls" | grep -q " libfoo1 libbar-2.0 ffmpeg$" && ! grep -q libssl-dev "$work/calls"'
check "--playwright: the dry run asks for Chromium and WebKit after apt-get update, in DIR" '[ "$(grep -m1 -n "apt-get .* update" "$work/calls" | cut -d: -f1)" -lt "$(grep -m1 -n "^playwright " "$work/calls" | cut -d: -f1)" ] && grep -q "^playwright install-deps --dry-run chromium webkit in $work/pw$" "$work/calls"'
check "--playwright: a second dry run checks that nothing is missing" '[ "$(grep -c "^playwright " "$work/calls")" -eq 2 ]'
rm -rf "$work"

# CI passes DIR relative to the checkout (crates/od-ui/e2e); the first CI
# run looked for the binary relative to DIR after changing into it.
setup_playwright
(cd "$work" && PATH="$work/bin:$PATH" APT_RETRY_SECONDS=0 "$script" --playwright pw > "$work/out" 2>&1)
rc=$?
check "--playwright: a DIR relative to the working directory" '[ $rc -eq 0 ] && grep "apt-get .* install" "$work/calls" | grep -q " libfoo1 libbar-2.0$"'
rm -rf "$work"

setup_playwright
PATH="$work/bin:$PATH" APT_RETRY_SECONDS=0 PW_MISSING_RUNS=2 "$script" --playwright "$work/pw" > "$work/out" 2>&1
rc=$?
check "--playwright: a package still missing after the install fails the step" '[ $rc -ne 0 ] && grep -q "^::error::linux-deps: after the install Playwright still misses: libfoo1 libbar-2.0$" "$work/out"'
rm -rf "$work"

setup_playwright
printf '#!/usr/bin/env bash\necho "something new"\nexit 1\n' > "$work/pw/node_modules/.bin/playwright"
PATH="$work/bin:$PATH" APT_RETRY_SECONDS=0 "$script" --playwright "$work/pw" > "$work/out" 2>&1
rc=$?
check "--playwright: output it cannot read stops before any install" '[ $rc -ne 0 ] && grep -q "cannot read the packages Playwright needs" "$work/out" && grep -q "something new" "$work/out" && ! grep -q "apt-get .* install" "$work/calls"'
rm -rf "$work"

setup
PATH="$work/bin:$PATH" APT_RETRY_SECONDS=0 "$script" --playwright "$work/none" > "$work/out" 2>&1
rc=$?
check "--playwright: without npm ci it says so and installs nothing" '[ $rc -ne 0 ] && grep -q "run npm ci in $work/none first" "$work/out" && ! grep -q "apt-get .* install" "$work/calls"'
rm -rf "$work"

[ "$failures" -eq 0 ] || { echo "$failures failed"; exit 1; }
