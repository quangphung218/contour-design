#!/usr/bin/env bash
# Installs the Debian/Ubuntu packages Tauri 2 needs to build the desktop app
# (and that `cargo build --workspace` needs, since od-desktop is a member).
# Usage: scripts/linux-deps.sh   (CI runs it; locally it asks for sudo)
set -euo pipefail
packages=(
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
  libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libxdo-dev libssl-dev
  build-essential pkg-config patchelf file
)
sudo apt-get update
sudo apt-get install -y --no-install-recommends "${packages[@]}"
