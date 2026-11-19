#!/usr/bin/env bash
# Decides which ci.yml jobs a change needs, from the files that differ
# between two commits. Prints, on stdout, lines for $GITHUB_OUTPUT:
#   kind=docs      only documentation: no build, no tests
#   kind=version   only what `make version` writes (scripts/version.sh): the
#                  [workspace.package] version in Cargo.toml, the same
#                  version of the workspace packages in Cargo.lock, a
#                  CHANGELOG.md with the new section; docs may change beside
#                  them. Nothing else: version.sh never writes
#                  apps/desktop/tauri.conf.json (it refuses a "version"
#                  there), so a change to it is code.
#                  The tag build runs every test before the release.
#   kind=code      anything else: lint, Linux tests, e2e, cargo-deny
#   windows=true   also the Windows tests (see WINDOWS below)
#   chrome=true    also the Chrome tests on macOS (see CHROME below)
# and on stderr, one line per decision saying why.
# A base that is empty, all zeros (a new branch) or missing from the clone
# runs everything. Never fails for a change it cannot read: it runs more.
# Usage: scripts/ci-changes.sh BASE HEAD   (in a git checkout holding both)
set -euo pipefail
base="${1-}"
head="${2:?usage: scripts/ci-changes.sh BASE HEAD}"

# Documentation: docs/ and Markdown, except Markdown the code reads
# (content/ skills and templates, crate and app fixtures, bridges/, the
# in-app guide crates/od-ui/docs/ that od-ui includes). A CLAUDE.md is
# notes for coding agents wherever it is: no code reads one (PR #264 added
# one per crate and ran every job twice for it).
DOCS='^(docs/|.*\.md$)'
DOCS_READ_BY_CODE='^(content|crates|apps|bridges)/'
AGENT_NOTES='(^|/)CLAUDE\.md$'
# Windows: the build inputs, the code the Windows job exists for (agent
# lookup through `.cmd` shims and the registry Path, the Job Object, the
# PTY, the desktop shell), and this classifier with its workflow. Any Rust
# file that branches on the platform is added below.
WINDOWS='^(apps/desktop/|crates/od-job/|crates/od-term/|crates/od-runtime/src/(detect|login_path|process|engine)\.rs$|Cargo\.(toml|lock)$|\.cargo/|rust-toolchain|(.*/)?build\.rs$|\.config/nextest\.toml$|\.github/workflows/ci\.yml$|scripts/ci-changes\.sh$)'
# `#[cfg(…)]`, `cfg!(…)` and `#[cfg_attr(…)]` naming windows or unix, and
# `target_os` / `target_family` = "windows".
PLATFORM_CFG='cfg(!|_attr)?\(.*(windows|unix)|target_(os|family) *= *"windows"'
# Chrome on macOS: how a render starts Chrome (od-render-cdp), the exports
# that use it (od-cli), a dependency change (chromiumoxide), the workflow.
CHROME='^(crates/od-(render-cdp|cli)/|Cargo\.lock$|\.github/workflows/ci\.yml$|scripts/ci-changes\.sh$)'

out() { printf '%s\n' "$@"; }
why() { printf '%s\n' "$*" >&2; }
everything() {
  why "$1: running every job"
  out kind=code windows=true chrome=true
  exit 0
}

[ -n "$base" ] && [ -z "$(printf '%s' "$base" | tr -d 0)" ] && base=""
[ -n "$base" ] || everything "no base commit"
git cat-file -e "$base^{commit}" 2>/dev/null || everything "base $base is not in this clone"
git cat-file -e "$head^{commit}" 2>/dev/null || everything "head $head is not in this clone"

# core.quotepath=off: names outside ASCII (`thử.rs`) as they are, not
# quoted and escaped, so the patterns below match them.
files="$(git -c core.quotepath=off diff --name-only --no-renames "$base" "$head")"
if [ -z "$files" ]; then
  why "docs: no file changed"
  out kind=docs windows=false chrome=false
  exit 0
fi

is_docs() { [[ "$1" =~ $AGENT_NOTES ]] || { [[ "$1" =~ $DOCS ]] && ! [[ "$1" =~ $DOCS_READ_BY_CODE ]]; }; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
# show REV PATH NAME: the file at REV into $work/NAME; fails if it is absent.
show() { git show "$1:$2" > "$work/$3" 2>/dev/null; }

# workspace_version FILE: [workspace.package] version of a Cargo.toml.
workspace_version() {
  awk '/^\[workspace\.package\]/{p=1;next} /^\[/{p=0} p && /^version *=/{gsub(/[" ]/,"");sub(/version=/,"");print;exit}' "$1"
}

# version_only: 0 when the change is exactly a version bump (see above).
version_only() {
  local f old new
  while IFS= read -r f; do
    case "$f" in
      Cargo.toml | Cargo.lock | CHANGELOG.md) ;;
      *) is_docs "$f" || { why "not version-only: $f changed"; return 1; } ;;
    esac
  done <<< "$files"
  printf '%s\n' "$files" | grep -qx Cargo.toml || { why "not version-only: Cargo.toml unchanged"; return 1; }
  show "$base" Cargo.toml base.toml && show "$head" Cargo.toml head.toml || return 1
  old="$(workspace_version "$work/base.toml")"
  new="$(workspace_version "$work/head.toml")"
  if [ -z "$old" ] || [ -z "$new" ] || [ "$old" = "$new" ]; then
    why "not version-only: [workspace.package] version did not change"
    return 1
  fi
  # Cargo.toml: the base with only that version changed.
  awk -v v="$new" '/^\[workspace\.package\]/{p=1} /^\[/ && !/^\[workspace\.package\]/{p=0}
    p && /^version *=/ {print "version = \"" v "\""; p=0; next} {print}' "$work/base.toml" > "$work/want.toml"
  cmp -s "$work/want.toml" "$work/head.toml" || { why "not version-only: Cargo.toml changed beyond the version"; return 1; }
  # Cargo.lock: the base with each workspace package (no `source`) at the
  # old version moved to the new one.
  if printf '%s\n' "$files" | grep -qx Cargo.lock; then
    show "$base" Cargo.lock base.lock && show "$head" Cargo.lock head.lock || return 1
    awk -v old="$old" -v new="$new" '
      function flush() { if (n) { for (i = 1; i <= n; i++) { if (!src && i == vi) print "version = \"" new "\""; else print buf[i] } } n = 0; vi = 0; src = 0 }
      /^\[\[package\]\]$/ { flush() }
      { buf[++n] = $0 }
      $0 == "version = \"" old "\"" { vi = n }
      /^source = / { src = 1 }
      END { flush() }' "$work/base.lock" > "$work/want.lock"
    cmp -s "$work/want.lock" "$work/head.lock" || { why "not version-only: Cargo.lock changed beyond the workspace version"; return 1; }
  fi
  # CHANGELOG.md: with the new version's section.
  show "$head" CHANGELOG.md head.md || return 1
  grep -qF "## [$new]" "$work/head.md" || { why "not version-only: CHANGELOG.md has no [$new] section"; return 1; }
  why "version-only: $old -> $new"
}

all_docs=1
while IFS= read -r f; do is_docs "$f" || { all_docs=0; break; }; done <<< "$files"
if [ "$all_docs" = 1 ]; then
  why "docs: only documentation changed"
  out kind=docs windows=false chrome=false
  exit 0
fi
if version_only; then
  out kind=version windows=false chrome=false
  exit 0
fi

windows=false chrome=false
while IFS= read -r f; do
  if [ "$windows" = false ]; then
    if [[ "$f" =~ $WINDOWS ]]; then
      windows=true; why "windows: $f"
    elif [[ "$f" == *.rs ]]; then
      for rev in "$base" "$head"; do
        if git show "$rev:$f" 2>/dev/null | grep -qE "$PLATFORM_CFG"; then
          windows=true; why "windows: $f branches on the platform"; break
        fi
      done
    fi
  fi
  if [ "$chrome" = false ] && [[ "$f" =~ $CHROME ]]; then
    chrome=true; why "chrome: $f"
  fi
done <<< "$files"
[ "$windows" = true ] || why "windows: no file the Windows tests are for"
[ "$chrome" = true ] || why "chrome: no file that starts Chrome"
out kind=code "windows=$windows" "chrome=$chrome"
