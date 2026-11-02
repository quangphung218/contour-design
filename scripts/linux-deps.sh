#!/usr/bin/env bash
# Installs the Debian/Ubuntu packages Tauri 2 needs to build the desktop app
# (and that `cargo build --workspace` needs, since od-desktop is a member).
# Usage: scripts/linux-deps.sh   (CI runs it; locally it asks for sudo)
#
# Each apt-get call gets at most APT_STEP_SECONDS (default 600) and three
# tries: on 02/11 a stalled mirror held the v0.2.0-beta.15 release job here
# for its full 90 minutes.
set -euo pipefail
packages=(
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
  libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libxdo-dev libssl-dev
  build-essential pkg-config patchelf file
)
step_seconds="${APT_STEP_SECONDS:-600}"

apt_get() {
  local try
  for try in 1 2 3; do
    if sudo timeout "$step_seconds" apt-get \
      -o Acquire::Retries=3 -o Acquire::http::Timeout=30 \
      -o Acquire::https::Timeout=30 -o DPkg::Lock::Timeout=120 "$@"; then
      return 0
    fi
    echo "linux-deps: apt-get $1 failed or timed out (try $try of 3)" >&2
    sleep $((try * ${APT_RETRY_SECONDS:-10}))
  done
  return 1
}

apt_get update
apt_get install -y --no-install-recommends "${packages[@]}"
