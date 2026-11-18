#!/usr/bin/env bash
# Installs the Debian/Ubuntu packages Tauri 2 needs to build the desktop app
# (and that `cargo build --workspace` needs, since od-desktop is a member).
# Usage: scripts/linux-deps.sh [PACKAGE…]   (CI runs it; locally it asks
# for sudo). Extra packages (CI: ffmpeg) go into the same install, under
# the same limits.
#
# Each apt-get call gets at most APT_STEP_SECONDS (default 600) and three
# tries: on 02/11 a stalled mirror held the v0.2.0-beta.15 release job here
# for its full 90 minutes.
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
step_seconds="${APT_STEP_SECONDS:-600}"
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

apt_get() {
  local try
  for try in 1 2 3; do
    if sudo timeout "$step_seconds" apt-get \
      -o Acquire::Retries=3 -o Acquire::http::Timeout=30 \
      -o Acquire::https::Timeout=30 -o DPkg::Lock::Timeout=120 \
      ${archives[@]+"${archives[@]}"} "$@"; then
      return 0
    fi
    echo "linux-deps: apt-get $1 failed or timed out (try $try of 3)" >&2
    sleep $((try * ${APT_RETRY_SECONDS:-10}))
  done
  return 1
}

apt_get update
apt_get install -y --no-install-recommends "${packages[@]}" "$@"
# Drop versions the mirror no longer has, so the folder does not grow.
[ -z "${APT_ARCHIVES:-}" ] || apt_get autoclean
