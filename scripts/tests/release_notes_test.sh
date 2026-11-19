#!/usr/bin/env bash
# Tests for scripts/release-notes.sh against a throwaway CHANGELOG.md: the
# plain section release.yml uses, and the --body release text (Highlights,
# What's changed, Verified, Downloads).
# Usage: scripts/tests/release_notes_test.sh   (or `make test-scripts`)
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
failures=0
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/scripts"
cp "$here/../release-notes.sh" "$work/scripts/release-notes.sh"
cat > "$work/CHANGELOG.md" <<'MD'
# Changelog

## [Unreleased]

### Changed

- Unreleased change.

## [1.2.0] - 2026-11-20

### Highlights

- Export waits for animations.
- Quitting asks first when agents run,
  and says how many.

### Added

- A new export.

### Fixed

- A fixed bug.

## [1.1.0] - 2026-11-10

### Fixed

- Fix one.
- Fix two.

### Added

- Add one.
- Add two.
- Add three.

## [1.0.0] - 2026-11-01

### Changed

- Only a change.

[Unreleased]: https://example.test/compare/v1.2.0...HEAD
[1.2.0]: https://example.test/compare/v1.1.0...v1.2.0
MD
run() { (cd "$work" && GITHUB_REPOSITORY= scripts/release-notes.sh "$@") > "$work/out" 2> "$work/err"; }
check() { # check <name> <condition>
  if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; sed 's/^/     /' "$work/out" "$work/err"; failures=$((failures + 1)); fi
}
has() { grep -qxF -- "$1" "$work/out"; }

run 1.2.0; rc=$?
check "plain: prints the section as it is" '[ $rc -eq 0 ] && has "### Highlights" && has "- A new export." && ! grep -q "## Downloads" "$work/out"'
check "plain: stops at the next version" '! grep -q "Fix one" "$work/out"'
run v1.2.0; rc=$?
check "plain: a leading v is accepted" '[ $rc -eq 0 ] && has "- A fixed bug."'
run 9.9.9; rc=$?
check "a version with no notes fails" '[ $rc -ne 0 ] && grep -q "no notes for 9.9.9" "$work/err"'
run 1.2.0 --verified x; rc=$?
check "--verified without --body is refused" '[ $rc -eq 2 ]'

run --body 1.2.0; rc=$?
check "body: the Highlights block comes first" '[ $rc -eq 0 ] && [ "$(head -1 "$work/out")" = "## Highlights" ] && has "- Export waits for animations."'
check "body: a wrapped highlight is one bullet" 'has "- Quitting asks first when agents run, and says how many."'
check "body: Highlights are not repeated under What's changed" '[ "$(grep -c "Export waits" "$work/out")" -eq 1 ] && [ "$(grep -c "^### Highlights" "$work/out")" -eq 0 ]'
check "body: What's changed keeps the section" 'has "## What'"'"'s changed" && has "### Added" && has "- A fixed bug."'
check "body: without --verified it says not checked by hand" 'grep -q "not yet checked by hand" "$work/out"'
check "body: every download row with the default repo" 'grep -qF "[\`Contour_1.2.0_universal.dmg\`](https://github.com/quangphung218/contour-design/releases/download/v1.2.0/Contour_1.2.0_universal.dmg)" "$work/out" && grep -qF "Contour_1.2.0_amd64.deb" "$work/out" && grep -qF "Contour_1.2.0_amd64.AppImage" "$work/out" && grep -qF "Contour_1.2.0_x64_en-US.msi" "$work/out"'
check "body: checksum and signature note" 'grep -qF "SHA256SUMS" "$work/out" && grep -qF "codesign --verify --deep --strict" "$work/out"'

run --body 1.1.0; rc=$?
check "body: without a Highlights block, the first Added then Fixed bullets, 3 at most" '[ $rc -eq 0 ] && [ "$(sed -n "3,5p" "$work/out" | tr "\n" "|")" = "- Add one.|- Add two.|- Add three.|" ]'
run --body 1.0.0; rc=$?
check "body: Changed when there is no Added or Fixed" '[ $rc -eq 0 ] && [ "$(sed -n 3p "$work/out")" = "- Only a change." ]'

printf 'Contour_1.2.0_universal.dmg\nContour_1.2.0_amd64.deb\nSHA256SUMS\n' > "$work/assets"
run --body 1.2.0 --verified "Checked by hand on macOS: [QA note](docs/qa/x.md)" --assets "$work/assets" --repo o/r; rc=$?
check "body: the maintainer's Verified line" '[ $rc -eq 0 ] && has "Checked by hand on macOS: [QA note](docs/qa/x.md)" && ! grep -q "not yet checked" "$work/out"'
check "body: --assets keeps only the files in the release" 'grep -qF "https://github.com/o/r/releases/download/v1.2.0/Contour_1.2.0_universal.dmg" "$work/out" && ! grep -q "x64_en-US.msi" "$work/out" && ! grep -q "AppImage\`" "$work/out"'
run --body 1.2.0 --assets "$work/missing"; rc=$?
check "body: a missing asset list fails" '[ $rc -ne 0 ] && grep -q "cannot read" "$work/err"'

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
echo "release-notes: all checks passed"
