#!/usr/bin/env bash
# Sets the release version. The only source is `[workspace.package] version`
# in Cargo.toml: every crate inherits it, the desktop bundle reads it (Tauri
# falls back to the Cargo version when tauri.conf.json has none), and the
# daemon reports it from CARGO_PKG_VERSION. This script also refreshes
# Cargo.lock and turns CHANGELOG.md "Unreleased" into a dated release.
#
# All checks run before anything is written. The new Cargo.toml and
# CHANGELOG.md are prepared in temporary files; if `cargo update` fails, the
# original Cargo.toml and Cargo.lock are restored and CHANGELOG.md is never
# touched, so the bump can simply be run again.
# Usage: scripts/version.sh 0.2.0   (or `make version V=0.2.0`)
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
v="${1:?usage: scripts/version.sh x.y.z}"
[[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "not a semver version: $v" >&2; exit 2; }
cd "$root"
repo="https://github.com/quangphung218/contour-design"
fail() { echo "$*" >&2; exit 1; }

# 1. Checks.
old="$(awk '/^\[workspace.package\]/{p=1;next} /^\[/{p=0} p && /^version *=/{gsub(/[" ]/,"",$0); sub(/version=/,""); print; exit}' Cargo.toml)"
[ -n "$old" ] || fail "Cargo.toml has no [workspace.package] version"
[ "$old" != "$v" ] || fail "already at $v"
if grep -q '"version"' apps/desktop/tauri.conf.json; then
  fail "apps/desktop/tauri.conf.json has a \"version\"; remove it so the bundle uses Cargo.toml"
fi
grep -q '^## \[Unreleased\]' CHANGELOG.md || fail "CHANGELOG.md has no '## [Unreleased]' section"
! grep -q "^## \[$v\]" CHANGELOG.md || fail "CHANGELOG.md already has a [$v] section"
unreleased="$(awk '/^## \[Unreleased\]/{p=1;next} p && (/^## \[/ || /^\[[^]]+\]: /){exit} p' CHANGELOG.md)"
[ -n "$(printf '%s' "$unreleased" | tr -d '[:space:]')" ] || fail "CHANGELOG.md: nothing under [Unreleased] to release"

# 2. Prepare every new file next to the original.
tmp="$(mktemp -d)"
restore=0
cleanup() {
  if [ "$restore" -eq 1 ]; then
    cp "$tmp/Cargo.toml.orig" Cargo.toml
    cp "$tmp/Cargo.lock.orig" Cargo.lock
    echo "restored Cargo.toml and Cargo.lock; nothing changed" >&2
  fi
  rm -rf "$tmp"
}
trap cleanup EXIT

awk -v v="$v" '/^\[workspace.package\]/{p=1} /^\[/ && !/^\[workspace.package\]/{p=0}
  p && /^version *=/ {print "version = \"" v "\""; p=0; next} {print}' Cargo.toml > "$tmp/Cargo.toml"
today="$(date +%Y-%m-%d)"
awk -v v="$v" -v d="$today" -v old="$old" -v repo="$repo" '
  /^## \[Unreleased\]/ && !done { print; print ""; print "## [" v "] - " d; done=1; next }
  /^\[Unreleased\]:/ { print "[Unreleased]: " repo "/compare/v" v "...HEAD"; print "[" v "]: " repo "/compare/v" old "...v" v; next }
  { print }' CHANGELOG.md > "$tmp/CHANGELOG.md"
cp Cargo.toml "$tmp/Cargo.toml.orig"
cp Cargo.lock "$tmp/Cargo.lock.orig"

# 3. Cargo.toml must be in place for `cargo update`; restore it if that fails.
restore=1
cp "$tmp/Cargo.toml" Cargo.toml
if ! { cargo update --workspace --offline --quiet 2>/dev/null || cargo update --workspace --quiet; }; then
  fail "cargo update failed; fix it and run the bump again"
fi
cp "$tmp/CHANGELOG.md" CHANGELOG.md
restore=0

echo "version $old -> $v (Cargo.toml, Cargo.lock, CHANGELOG.md)"
echo "next: review the [$v] notes, commit 'build(release): v$v', merge, then tag v$v (docs/release.md)"
