#!/usr/bin/env bash
# Copies a built Contour.app into /Applications. Replaces /Applications/Contour.app
# only when it is Contour (same bundle identifier); never touches other apps.
# Usage: scripts/install-app.sh <path/to/Contour.app>   (or `make install` after `make app-release`)
set -euo pipefail
src="${1:?usage: scripts/install-app.sh <path/to/Contour.app>}"
dest_dir="${INSTALL_DIR:-/Applications}"
[ -d "$src" ] || { echo "no app at $src; run make app-release first" >&2; exit 1; }

plist_id() { /usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$1/Contents/Info.plist" 2>/dev/null; }
id="$(plist_id "$src")"
[ -n "$id" ] || { echo "$src has no CFBundleIdentifier" >&2; exit 1; }
dest="$dest_dir/$(basename "$src")"

if [ -e "$dest" ]; then
  existing="$(plist_id "$dest" || true)"
  if [ "$existing" != "$id" ]; then
    echo "refusing to replace $dest: its bundle id is '${existing:-unknown}', not '$id'" >&2
    exit 1
  fi
  pgrep -f "$dest/Contents/MacOS/" >/dev/null && { echo "quit $(basename "$dest") first" >&2; exit 1; }
  rm -rf "$dest"
fi
ditto "$src" "$dest"
echo "installed $dest ($(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$dest/Contents/Info.plist"))"
