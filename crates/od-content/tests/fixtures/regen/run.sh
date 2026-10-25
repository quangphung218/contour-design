#!/usr/bin/env bash
# Regenerates original-skills.json and original-design-templates.json with
# the original app's loader. Usage: run.sh [path to open-design at e03d5c2]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
od="${1:-$HOME/Documents/GitHub/open-design}"
content="$(cd "$here/../../../../../content" && pwd)"
[ "$(git -C "$od" rev-parse --short=7 HEAD)" = e03d5c2 ] || { echo "open-design must be at e03d5c2" >&2; exit 1; }
run() { node --import "$here/hooks.mjs" --experimental-strip-types "$here/skills.mjs" "$od" "$content/$1" > "$here/../original-$1.json"; }
run skills
run design-templates
