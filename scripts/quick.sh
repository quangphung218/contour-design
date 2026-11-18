#!/usr/bin/env bash
# `make quick`: the fast check while you work. fmt for everything, then
# clippy and the tests of only the crates your branch changes (against
# origin/main, plus uncommitted and untracked files) and the workspace
# crates that depend on them directly. `make check` stays the gate before
# review, and CI runs the rest (docs/development.md, Which check when).
#
# content/ and bridges/ check the crates whose tests read them. A change
# to the build itself (Cargo.toml, Cargo.lock, .cargo/,
# rust-toolchain) checks every crate. A change to no crate runs fmt only.
# Uses cargo-nextest when installed, `cargo test` otherwise.
#   QUICK_BASE=<ref>   compare with another ref than origin/main
# Usage: scripts/quick.sh               (or `make quick`)
#        scripts/quick.sh --packages FILE…   print the crates FILEs select
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

# packages FILE…: the workspace packages to check, one per line; "*" for all.
packages() {
  cargo metadata --format-version 1 --no-deps --offline 2>/dev/null |
    python3 -c '
import json, os, sys
meta = json.load(sys.stdin)
root = os.path.realpath(sys.argv[1])
files = sys.argv[2:]
build = ("Cargo.toml", "Cargo.lock", "rust-toolchain", "rust-toolchain.toml")
if any(f in build or f.startswith(".cargo/") for f in files):
    print("*"); sys.exit()
dirs = {}
for p in meta["packages"]:
    d = os.path.relpath(os.path.dirname(os.path.realpath(p["manifest_path"])), root)
    dirs[d] = p["name"]
changed = set()
for f in files:
    for d, name in dirs.items():
        if f.startswith(d + "/"):
            changed.add(name)
picked = set(changed)
for p in meta["packages"]:
    if any(dep.get("path") and dep["name"] in changed for dep in p["dependencies"]):
        picked.add(p["name"])
# Files outside the crates that their tests read.
readers = {
    "content/": ["od-content", "od-preview", "od-cli", "od-server"],
    "bridges/": ["od-preview", "od-render-cdp"],
}
for prefix, names in readers.items():
    if any(f.startswith(prefix) for f in files):
        picked.update(names)
print("\n".join(sorted(picked)))
' "$root" "$@"
}

if [ "${1-}" = --packages ]; then
  shift
  packages "$@"
  exit 0
fi

base_ref="${QUICK_BASE:-origin/main}"
base="$(git merge-base "$base_ref" HEAD)" || { echo "quick: no merge base with $base_ref (git fetch origin?)" >&2; exit 2; }
files=()
while IFS= read -r f; do [ -z "$f" ] || files+=("$f"); done < <(
  { git diff --name-only "$base"; git ls-files --others --exclude-standard; } | sort -u)

started=$(date +%s)
echo "quick: ${#files[@]} file(s) changed since $base_ref"
cargo fmt --all --check

picked=()
while IFS= read -r p; do [ -z "$p" ] || picked+=("$p"); done < <(packages ${files[@]+"${files[@]}"})
if [ "${#picked[@]}" -eq 0 ]; then
  echo "quick: no crate changed; fmt only ($(($(date +%s) - started)) s)"
  exit 0
fi
if [ "${picked[0]}" = "*" ]; then
  echo "quick: the build changed (Cargo.toml, Cargo.lock, .cargo or the toolchain): every crate"
  args=(--workspace)
else
  echo "quick: ${picked[*]}"
  args=()
  for p in "${picked[@]}"; do args+=(-p "$p"); done
fi

cargo clippy "${args[@]}" --all-targets -- -D warnings
if [ "${picked[0]}" = "*" ] || [[ " ${picked[*]} " == *" od-ui "* ]]; then
  cargo clippy -p od-ui --target wasm32-unknown-unknown -- -D warnings
fi
if cargo nextest --version > /dev/null 2>&1; then
  cargo nextest run "${args[@]}"
else
  cargo test "${args[@]}"
fi
echo "quick: passed in $(($(date +%s) - started)) s; \`make check\` before review"
