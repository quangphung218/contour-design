#!/usr/bin/env bash
# Keeps CHANGELOG entries to one short line each, so the release page reads
# as a list a user can scan (CONTRIBUTING.md, "Changelog"). Every bullet above
# the `<!-- changelog-lint: stop -->` marker must:
#   - fit on one line (no wrapped continuation lines),
#   - have at most MAX_WORDS words (default 25), PR references not counted.
# Details belong in the pull request and in docs/, not in the CHANGELOG.
# Usage: scripts/changelog-lint.sh [CHANGELOG.md]
set -uo pipefail
file="${1:-CHANGELOG.md}"
max="${MAX_WORDS:-25}"
awk -v max="$max" -v file="$file" '
  /<!-- changelog-lint: stop -->/ { exit }
  /^## / { section = $0; inbullet = 0; next }
  /^- / {
    inbullet = 1; line = NR
    text = substr($0, 3)
    gsub(/\(#[0-9]+(, #[0-9]+)*\)/, "", text)
    n = split(text, w, /[ \t]+/); words = 0
    for (i = 1; i <= n; i++) if (w[i] ~ /[[:alnum:]]/) words++
    if (words > max) {
      printf "%s:%d: %d words (max %d): %s\n", file, NR, words, max, substr($0, 1, 80) "…"
      bad++
    }
    next
  }
  /^[ \t]+[^ \t]/ && inbullet {
    printf "%s:%d: a CHANGELOG entry is one line; continuation found\n", file, NR
    bad++; next
  }
  /^$/ { inbullet = 0 }
  END {
    if (bad) {
      print "changelog-lint: " bad " problem(s). One short line per change: what a user notices, starting with a verb, ending with (#PR). Details go in the PR." > "/dev/stderr"
      exit 1
    }
  }
' "$file"
