#!/usr/bin/env bash
# Tests for scripts/version.sh in a throwaway copy of the files it edits,
# with a fake `cargo` on PATH. A failed bump must leave every file as it was.
# Usage: scripts/tests/version_test.sh   (or `make test-scripts`)
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../version.sh"
failures=0

setup() {
  work="$(mktemp -d)"
  mkdir -p "$work/scripts" "$work/apps/desktop" "$work/bin"
  cp "$script" "$work/scripts/version.sh"
  printf '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "0.1.0"\nedition = "2024"\n' > "$work/Cargo.toml"
  printf 'lock for 0.1.0\n' > "$work/Cargo.lock"
  printf '{\n  "productName": "Contour"\n}\n' > "$work/apps/desktop/tauri.conf.json"
  cat > "$work/CHANGELOG.md" <<'MD'
# Changelog

## [Unreleased]

### Fixed

- Something.

## [0.1.0] - 2026-10-24

- First.

[Unreleased]: https://example.test/compare/v0.1.0...HEAD
[0.1.0]: https://example.test/releases/tag/v0.1.0
MD
  # Fake cargo: FAKE_CARGO=fail fails, otherwise rewrites the lock file.
  cat > "$work/bin/cargo" <<'SH'
#!/bin/sh
[ "${FAKE_CARGO:-ok}" = fail ] && { echo "fake cargo: network down" >&2; exit 101; }
echo "lock updated" > Cargo.lock
SH
  chmod +x "$work/bin/cargo"
}

snapshot() { (cd "$work" && cat Cargo.toml Cargo.lock CHANGELOG.md apps/desktop/tauri.conf.json | shasum); }
run() { (cd "$work" && PATH="$work/bin:$PATH" scripts/version.sh "$@") >"$work/out" 2>&1; }
check() {
  if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; sed 's/^/     /' "$work/out"; failures=$((failures + 1)); fi
}
no_leftovers() { [ -z "$(cd "$work" && find . -name '*.tmp' -o -name '*.bak' | head -1)" ]; }

setup
printf '{\n  "productName": "Contour",\n  "version": "9.9.9"\n}\n' > "$work/apps/desktop/tauri.conf.json"
before="$(snapshot)"; run 0.2.0; rc=$?
check "a version in tauri.conf.json stops the bump before any write" '[ $rc -ne 0 ] && [ "$(snapshot)" = "$before" ] && no_leftovers'
rm -rf "$work"

setup
before="$(snapshot)"; FAKE_CARGO=fail run 0.2.0; rc=$?
check "a failing cargo update leaves Cargo.toml, Cargo.lock and CHANGELOG.md as they were" '[ $rc -ne 0 ] && [ "$(snapshot)" = "$before" ] && no_leftovers'
FAKE_CARGO=ok run 0.2.0; rc=$?
check "the same bump succeeds once cargo works again" '[ $rc -eq 0 ] && grep -q "^version = \"0.2.0\"" "$work/Cargo.toml"'
rm -rf "$work"

setup
perl -0 -i -pe 's/### Fixed\n\n- Something.\n\n//' "$work/CHANGELOG.md"
before="$(snapshot)"; run 0.2.0; rc=$?
check "an empty Unreleased section stops the bump" '[ $rc -ne 0 ] && [ "$(snapshot)" = "$before" ]'
rm -rf "$work"

setup
run 0.2.0; rc=$?
check "a bump updates Cargo.toml, Cargo.lock and CHANGELOG.md" '[ $rc -eq 0 ] \
  && grep -q "^version = \"0.2.0\"" "$work/Cargo.toml" && grep -q "edition = \"2024\"" "$work/Cargo.toml" \
  && grep -q "lock updated" "$work/Cargo.lock" \
  && grep -q "^## \[0.2.0\] - " "$work/CHANGELOG.md" && grep -q "^## \[Unreleased\]" "$work/CHANGELOG.md" \
  && grep -q "compare/v0.2.0...HEAD" "$work/CHANGELOG.md" && no_leftovers'
run 0.2.0; rc=$?
check "bumping to the current version is refused" '[ $rc -ne 0 ]'
run 1.x; rc=$?
check "a non-semver version is refused" '[ $rc -eq 2 ]'
rm -rf "$work"

[ "$failures" -eq 0 ] && echo "version.sh: all checks passed" || { echo "version.sh: $failures failed"; exit 1; }
