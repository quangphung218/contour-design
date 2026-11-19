#!/usr/bin/env bash
# Installs the Debian/Ubuntu packages Tauri 2 needs to build the desktop app
# (and that `cargo build --workspace` needs, since od-desktop is a member).
# Usage: scripts/linux-deps.sh [--playwright DIR] [PACKAGE…]
# (CI runs it; locally it asks for sudo). Extra packages (CI: ffmpeg) go
# into the same install, under the same limits.
#
# --playwright DIR: instead of the Tauri packages, the ones Playwright's
# Chromium and WebKit still miss, as `playwright install-deps --dry-run`
# in DIR (after `npm ci`) lists them; afterwards it must list none. CI's
# e2e job uses it in place of `playwright install --with-deps`, whose own
# apt-get has no time limit, no retry and no .deb cache: on 18-19/11 it
# took 35 to 44 minutes for 126 MB and the job ran out of time.
#
# Each apt-get call gets at most APT_STEP_SECONDS (default 600) and three
# tries: on 02/11 a stalled mirror held the v0.2.0-beta.15 release job here
# for its full 90 minutes. APT_BUDGET_SECONDS caps all of them together
# (default: no cap): when it runs out the script stops with an error that
# names the mirror, instead of using up the job's time.
#
# APT_ARCHIVES=DIR keeps the downloaded .deb files in DIR (CI caches it):
# a package already there is not downloaded again, and a download cut by
# the step limit goes on from where it stopped. On 18/11 a mirror at about
# 50 kB/s held 7 of 12 Linux CI jobs for 9 to 31 minutes in apt (128 MB).
# On exit, also after a failure, DIR belongs to the user again and, when
# GITHUB_OUTPUT is set, `archives-changed=true|false` says whether DIR
# changed (CI saves it only then).
set -euo pipefail
packages=(
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
  libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libxdo-dev libssl-dev
  build-essential pkg-config patchelf file
)
playwright_dir=""
if [ "${1-}" = --playwright ]; then
  playwright_dir="${2:?usage: scripts/linux-deps.sh [--playwright DIR] [PACKAGE…]}"
  shift 2
  packages=()
fi
# The browsers the e2e job installs (.github/workflows/ci.yml).
playwright_browsers=(chromium webkit)
step_seconds="${APT_STEP_SECONDS:-600}"
budget="${APT_BUDGET_SECONDS:-}"
started=$SECONDS
archives=()
if [ -n "${APT_ARCHIVES:-}" ]; then
  mkdir -p "$APT_ARCHIVES/partial"
  archives=(-o "Dir::Cache::Archives=$APT_ARCHIVES")
  # listing: every file in DIR with its size, to tell whether apt changed it.
  listing() { (cd "$APT_ARCHIVES" && find . -type f ! -name lock -exec ls -ln {} + 2>/dev/null | awk '{print $5, $NF}' | sort); }
  before="$(listing)"
  finish() {
    # apt ran as root; the cache step reads and later restores the folder.
    sudo rm -f "$APT_ARCHIVES/lock"
    sudo chown -R "$(id -u):$(id -g)" "$APT_ARCHIVES"
    if [ -n "${GITHUB_OUTPUT:-}" ]; then
      if [ "$(listing)" = "$before" ]; then
        echo "archives-changed=false" >> "$GITHUB_OUTPUT"
      else
        echo "archives-changed=true" >> "$GITHUB_OUTPUT"
      fi
    fi
  }
  trap finish EXIT
fi

# out_of_budget STEP: stops the script; the message is for the CI log.
out_of_budget() {
  local kept=""
  [ -z "${APT_ARCHIVES:-}" ] || kept=" The .deb files downloaded so far stay in the cache, so the next run goes on from there."
  echo "::error::linux-deps: apt-get $1 did not finish within APT_BUDGET_SECONDS=$budget: the Ubuntu package mirror is slow or down, not this change. Run the job again.$kept" >&2
  exit 1
}

apt_get() {
  local try limit left
  for try in 1 2 3; do
    limit="$step_seconds"
    if [ -n "$budget" ]; then
      left=$((budget - (SECONDS - started)))
      [ "$left" -gt 0 ] || out_of_budget "$1"
      [ "$left" -ge "$limit" ] || limit="$left"
    fi
    if sudo timeout "$limit" apt-get \
      -o Acquire::Retries=3 -o Acquire::http::Timeout=30 \
      -o Acquire::https::Timeout=30 -o DPkg::Lock::Timeout=120 \
      ${archives[@]+"${archives[@]}"} "$@"; then
      return 0
    fi
    echo "linux-deps: apt-get $1 failed or timed out (try $try of 3)" >&2
    sleep $((try * ${APT_RETRY_SECONDS:-10}))
  done
  [ -z "$budget" ] || [ $((budget - (SECONDS - started))) -gt 0 ] || out_of_budget "$1"
  return 1
}

# playwright_missing: the packages Playwright's browsers still need, one a
# line (none when all are there); fails when it cannot tell.
playwright_missing() {
  local bin="$playwright_dir/node_modules/.bin/playwright" out
  [ -x "$bin" ] || { echo "linux-deps: no $bin; run npm ci in $playwright_dir first" >&2; return 1; }
  # Exit 1 with a list when packages are missing, 0 when none are.
  out="$(cd "$playwright_dir" && "$bin" install-deps --dry-run "${playwright_browsers[@]}" 2>&1)" || true
  if grep -q '^All system dependencies are installed' <<< "$out"; then
    return 0
  elif grep -q '^Missing system dependencies' <<< "$out"; then
    sed -n '/^Missing system dependencies/,$ s/^  \([^ ][^ ]*\)$/\1/p' <<< "$out"
  else
    printf '%s\n' "$out" >&2
    echo "linux-deps: cannot read the packages Playwright needs from the output above" >&2
    return 1
  fi
}

apt_get update
if [ -n "$playwright_dir" ]; then
  missing="$(playwright_missing)"
  # Package names hold no spaces.
  # shellcheck disable=SC2206
  packages=($missing)
  echo "linux-deps: Playwright (${playwright_browsers[*]}) needs ${#packages[@]} package(s)"
fi
apt_get install -y --no-install-recommends ${packages[@]+"${packages[@]}"} "$@"
if [ -n "$playwright_dir" ]; then
  missing="$(playwright_missing)"
  if [ -n "$missing" ]; then
    echo "::error::linux-deps: after the install Playwright still misses: ${missing//$'\n'/ }" >&2
    exit 1
  fi
fi
# Drop versions the mirror no longer has, so the folder does not grow.
[ -z "${APT_ARCHIVES:-}" ] || apt_get autoclean
