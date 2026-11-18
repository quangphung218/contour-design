#!/usr/bin/env bash
# Tests for scripts/ci-changes.sh in a throwaway git repository: which
# changes count as docs, as a version bump, and which pull in the Windows
# and Chrome jobs.
# Usage: scripts/tests/ci_changes_test.sh   (or `make test-scripts`)
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../ci-changes.sh"
repo="$(mktemp -d)"
why="$(mktemp)"
trap 'rm -rf "$repo" "$why"' EXIT
failures=0

git -C "$repo" init -q -b main
git -C "$repo" config user.email test@example.test
git -C "$repo" config user.name test
git -C "$repo" config commit.gpgsign false
mkdir -p "$repo/crates/od-server/src" "$repo/crates/od-render-cdp/src" "$repo/crates/od-ui/src" \
  "$repo/apps/desktop" "$repo/docs" "$repo/content/skills/a"
cat > "$repo/Cargo.toml" <<'EOF'
[workspace]
members = ["crates/*"]

[workspace.package]
version = "0.2.0-beta.1"
edition = "2024"

[workspace.dependencies]
serde = { version = "1" }
EOF
cat > "$repo/Cargo.lock" <<'EOF'
version = 4

[[package]]
name = "od-server"
version = "0.2.0-beta.1"
dependencies = [
 "serde",
]

[[package]]
name = "serde"
version = "0.2.0-beta.1"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "zeta"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
EOF
printf '# Changelog\n\n## [Unreleased]\n\n- A fix.\n\n## [0.2.0-beta.1] - 2026-11-01\n' > "$repo/CHANGELOG.md"
printf '{\n  "productName": "Contour"\n}\n' > "$repo/apps/desktop/tauri.conf.json"
printf 'pub fn a() {}\n' > "$repo/crates/od-server/src/lib.rs"
printf '#[cfg(windows)]\npub fn w() {}\n' > "$repo/crates/od-server/src/win.rs"
printf 'pub fn r() {}\n' > "$repo/crates/od-render-cdp/src/lib.rs"
printf 'pub fn ui() {}\n' > "$repo/crates/od-ui/src/lib.rs"
printf '# Docs\n' > "$repo/docs/a.md"
printf '# Skill\n' > "$repo/content/skills/a/SKILL.md"
git -C "$repo" add -A
git -C "$repo" commit -qm base
root="$(git -C "$repo" rev-parse HEAD)"

# change NAME: resets to the base commit; the caller edits files, then `commit`.
change() { git -C "$repo" checkout -q -B "$1" "$root"; }
commit() { git -C "$repo" add -A && git -C "$repo" commit -qm change --allow-empty; }
# expect NAME WANT [BASE]: the script's stdout for base..HEAD, one line.
expect() {
  local got
  got="$(cd "$repo" && "$script" "${3-$root}" HEAD 2>"$why" | tr '\n' ' ')"
  if [ "$got" = "$2 " ]; then
    echo "ok   $1"
  else
    echo "FAIL $1"
    echo "     want: $2"
    echo "     got:  $got"
    sed 's/^/     /' "$why"
    failures=$((failures + 1))
  fi
}
# bump: what scripts/version.sh writes for 0.2.0-beta.2.
bump() {
  sed -i.bak 's/^version = "0.2.0-beta.1"$/version = "0.2.0-beta.2"/' "$repo/Cargo.toml"
  awk 'prev == "name = \"od-server\"" && /^version/ {print "version = \"0.2.0-beta.2\""; prev=$0; next} {print; prev=$0}' \
    "$repo/Cargo.lock" > "$repo/Cargo.lock.new" && mv "$repo/Cargo.lock.new" "$repo/Cargo.lock"
  sed -i.bak 's/^## \[Unreleased\]$/## [Unreleased]\n\n## [0.2.0-beta.2] - 2026-11-18/' "$repo/CHANGELOG.md"
  if ! grep -q '0.2.0-beta.2' "$repo/CHANGELOG.md"; then
    # BSD sed: no \n in the replacement.
    awk '{print} /^## \[Unreleased\]$/ {print ""; print "## [0.2.0-beta.2] - 2026-11-18"}' "$repo/CHANGELOG.md.bak" > "$repo/CHANGELOG.md"
  fi
  rm -f "$repo"/*.bak
}

ALL="kind=code windows=true chrome=true"
NONE_CODE="kind=code windows=false chrome=false"

change version; bump; commit
expect "make version output is version-only" "kind=version windows=false chrome=false"

change version-docs; bump; printf '# Docs\nmore\n' > "$repo/docs/a.md"; commit
expect "a version bump with a docs edit is version-only" "kind=version windows=false chrome=false"

change version-notes; bump; printf -- '- Notes reworded.\n' >> "$repo/CHANGELOG.md"; commit
expect "release notes edited in the bump stay version-only" "kind=version windows=false chrome=false"

change version-dep; bump; sed -i.bak 's/^version = "1.0.0"$/version = "1.0.1"/' "$repo/Cargo.lock"; rm -f "$repo"/*.bak; commit
expect "a bump plus a dependency change in Cargo.lock is code" "$ALL"

change version-registry; bump; sed -i.bak '/name = "serde"/{n;s/0.2.0-beta.1/0.2.0-beta.2/;}' "$repo/Cargo.lock"; rm -f "$repo"/*.bak; commit
expect "a registry package at the old version moved too is code" "$ALL"

change version-toml; bump; sed -i.bak 's/^edition = "2024"$/edition = "2021"/' "$repo/Cargo.toml"; rm -f "$repo"/*.bak; commit
expect "a bump plus another Cargo.toml change is code" "$ALL"

change version-dep-toml; bump; sed -i.bak 's/version = "1" }/version = "2" }/' "$repo/Cargo.toml"; rm -f "$repo"/*.bak; commit
expect "a bump plus a dependency version in Cargo.toml is code" "$ALL"

change version-no-changelog; bump; git -C "$repo" checkout -q "$root" -- CHANGELOG.md; commit
expect "a bump without its CHANGELOG section is code" "$ALL"

change version-no-section; bump; printf '# Changelog\n\n## [Unreleased]\n\n- A fix.\n' > "$repo/CHANGELOG.md"; commit
expect "a bump whose CHANGELOG lacks the new section is code" "$ALL"

change version-code; bump; printf 'pub fn a() { }\n' > "$repo/crates/od-server/src/lib.rs"; commit
expect "a bump plus code is code" "$ALL"

change version-tauri; bump
printf '{\n  "productName": "Contour",\n  "version": "0.2.0-beta.2"\n}\n' > "$repo/apps/desktop/tauri.conf.json"; commit
expect "a version line in tauri.conf.json is version-only" "kind=version windows=false chrome=false"

change version-tauri-more; bump; printf '{\n  "productName": "Contour 2"\n}\n' > "$repo/apps/desktop/tauri.conf.json"; commit
expect "another tauri.conf.json change is code" "$ALL"

change docs; printf '# Docs\nmore\n' > "$repo/docs/a.md"; printf 'x\n' > "$repo/README.md"; commit
expect "docs and root Markdown are docs" "kind=docs windows=false chrome=false"

change changelog; printf -- '- Another.\n' >> "$repo/CHANGELOG.md"; commit
expect "a CHANGELOG line alone is docs" "kind=docs windows=false chrome=false"

change skill; printf '# Skill 2\n' > "$repo/content/skills/a/SKILL.md"; commit
expect "Markdown under content/ is code (the content tests read it)" "$NONE_CODE"

change ui; printf 'pub fn ui() { }\n' > "$repo/crates/od-ui/src/lib.rs"; commit
expect "a UI change runs neither Windows nor Chrome" "$NONE_CODE"

change plain; printf 'pub fn a() { }\n' > "$repo/crates/od-server/src/lib.rs"; commit
expect "a Rust file without a platform cfg runs no Windows tests" "$NONE_CODE"

change cfg; printf '#[cfg(windows)]\npub fn w() { }\n' > "$repo/crates/od-server/src/win.rs"; commit
expect "a Rust file with a platform cfg runs the Windows tests" "kind=code windows=true chrome=false"

change cfg-removed; printf 'pub fn w() {}\n' > "$repo/crates/od-server/src/win.rs"; commit
expect "removing a platform cfg runs the Windows tests" "kind=code windows=true chrome=false"

change cfg-deleted; git -C "$repo" rm -q crates/od-server/src/win.rs; commit
expect "deleting a file with a platform cfg runs the Windows tests" "kind=code windows=true chrome=false"

change desktop; printf 'x\n' > "$repo/apps/desktop/main.rs"; commit
expect "apps/desktop runs the Windows tests" "kind=code windows=true chrome=false"

change render; printf 'pub fn r() { }\n' > "$repo/crates/od-render-cdp/src/lib.rs"; commit
expect "od-render-cdp runs the Chrome tests" "kind=code windows=false chrome=true"

change spaces; printf 'x\n' > "$repo/content/skills/a/my notes.txt"; commit
expect "a file name with spaces" "$NONE_CODE"

change empty; commit
expect "no change is docs" "kind=docs windows=false chrome=false"

change any; printf 'pub fn a() { }\n' > "$repo/crates/od-server/src/lib.rs"; commit
expect "an empty base runs everything" "$ALL" ""
expect "an all-zero base (new branch) runs everything" "$ALL" 0000000000000000000000000000000000000000
if grep -q "^no base commit" "$why"; then echo "ok   an all-zero base is read as no base"; else echo "FAIL an all-zero base is read as no base: $(cat "$why")"; failures=$((failures + 1)); fi
expect "a base missing from the clone runs everything" "$ALL" 1234567890abcdef1234567890abcdef12345678

[ "$failures" -eq 0 ] || { echo "ci-changes: $failures check(s) failed" >&2; exit 1; }
echo "ci-changes: all checks passed"
