#!/usr/bin/env bash
# Sets the release version. The only source is `[workspace.package] version`
# in Cargo.toml: every crate inherits it, the desktop bundle reads it (Tauri
# falls back to the Cargo version when tauri.conf.json has none), and the
# daemon reports it from CARGO_PKG_VERSION. This script also refreshes
# Cargo.lock and turns CHANGELOG.md "Unreleased" into a dated release.
# Usage: scripts/version.sh 0.2.0   (or `make version V=0.2.0`)
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
v="${1:?usage: scripts/version.sh x.y.z}"
[[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "not a semver version: $v" >&2; exit 2; }
cd "$root"

old="$(awk '/^\[workspace.package\]/{p=1;next} /^\[/{p=0} p && /^version *=/{gsub(/[" ]/,"",$0); sub(/version=/,""); print; exit}' Cargo.toml)"
[ "$old" != "$v" ] || { echo "already at $v" >&2; exit 1; }
grep -q '^## \[Unreleased\]' CHANGELOG.md || { echo "CHANGELOG.md has no '## [Unreleased]' section" >&2; exit 1; }
grep -q "^## \[$v\]" CHANGELOG.md && { echo "CHANGELOG.md already has a [$v] section" >&2; exit 1; }
unreleased="$(awk '/^## \[Unreleased\]/{p=1;next} p && (/^## \[/ || /^\[[^]]+\]: /){exit} p' CHANGELOG.md)"
[ -n "$(printf '%s' "$unreleased" | tr -d '[:space:]')" ] || { echo "CHANGELOG.md: nothing under [Unreleased] to release" >&2; exit 1; }

# Cargo.toml: only the version line inside [workspace.package].
awk -v v="$v" '/^\[workspace.package\]/{p=1} /^\[/ && !/^\[workspace.package\]/{p=0}
  p && /^version *=/ {print "version = \"" v "\""; p=0; next} {print}' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml
# tauri.conf.json carries no version on purpose; reject one that crept back.
if grep -q '"version"' apps/desktop/tauri.conf.json; then
  echo "apps/desktop/tauri.conf.json has a \"version\"; remove it so the bundle uses Cargo.toml" >&2; exit 1
fi
cargo update --workspace --offline --quiet 2>/dev/null || cargo update --workspace --quiet

# CHANGELOG.md: new empty Unreleased on top, the old one becomes [v] - today,
# and the compare links at the bottom move along.
today="$(date +%Y-%m-%d)"
repo="https://github.com/quangphung218/contour-design"
awk -v v="$v" -v d="$today" -v old="$old" -v repo="$repo" '
  /^## \[Unreleased\]/ && !done { print; print ""; print "## [" v "] - " d; done=1; next }
  /^\[Unreleased\]:/ { print "[Unreleased]: " repo "/compare/v" v "...HEAD"; print "[" v "]: " repo "/compare/v" old "...v" v; next }
  { print }' CHANGELOG.md > CHANGELOG.md.tmp && mv CHANGELOG.md.tmp CHANGELOG.md

echo "version $old -> $v (Cargo.toml, Cargo.lock, CHANGELOG.md)"
echo "next: review the [$v] notes, commit 'build(release): v$v', merge, then tag v$v (docs/release.md)"
