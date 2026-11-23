#!/usr/bin/env bash
# New CHANGELOG entries go under `## [Unreleased]`. A branch made before a
# release that merges main can land its line in the released section instead
# (#231, #260, beta.47): the line then never reaches the next release page.
# Fails when a bullet this branch adds sits outside [Unreleased].
# Usage: scripts/changelog-placement.sh <base> <head> [CHANGELOG.md]
set -uo pipefail
base="${1:?base}"; head="${2:?head}"; file="${3:-CHANGELOG.md}"
added="$(git diff -U0 "$base...$head" -- "$file" | awk '
  /^@@/ { split($3, a, ","); sub(/^\+/, "", a[1]); n = a[1]; next }
  /^\+\+\+/ { next }
  /^\+/ { if (substr($0, 2, 2) == "- ") print n; n++; next }
')"
[ -z "$added" ] && exit 0
range="$(git show "$head:$file" | awk '
  /^## \[Unreleased\]/ { s = NR; next }
  s && /^## \[/ { print s, NR; found = 1; exit }
  END { if (s && !found) print s, NR + 1 }
')"
read -r from to <<< "${range:-0 0}"
bad=0
for n in $added; do
  if [ "$n" -le "$from" ] || [ "$n" -ge "$to" ]; then
    echo "$file:$n: entry added outside [Unreleased]: $(git show "$head:$file" | sed -n "${n}p" | cut -c1-80)"
    bad=$((bad + 1))
  fi
done
[ "$bad" -eq 0 ] || { echo "changelog-placement: move the line(s) under ## [Unreleased] (a merge of main put them in a released section)." >&2; exit 1; }
