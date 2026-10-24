#!/usr/bin/env bash
# Prints the CHANGELOG.md section of one version (the release body).
# Usage: scripts/release-notes.sh 0.1.0   (a leading "v" is accepted)
set -euo pipefail
v="${1:?usage: scripts/release-notes.sh x.y.z}"
v="${v#v}"
file="$(cd "$(dirname "$0")/.." && pwd)/CHANGELOG.md"
notes="$(awk -v v="$v" '
  index($0, "## [" v "]") == 1 { p=1; next }
  p && (/^## \[/ || /^\[[^]]+\]: /) { exit }
  p { print }' "$file")"
[ -n "$(printf '%s' "$notes" | tr -d '[:space:]')" ] || { echo "CHANGELOG.md has no notes for $v" >&2; exit 1; }
printf '%s\n' "$notes"
