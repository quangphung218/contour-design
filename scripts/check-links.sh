#!/usr/bin/env bash
# Fails when a relative link or image in the repo's Markdown points at a file
# or heading that does not exist. External (http, mailto) links are skipped:
# the check never touches the network.
# Usage: scripts/check-links.sh   (or `make links`)
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
git ls-files -z --cached --others --exclude-standard -- '*.md' ':!content/**' ':!.claude/**' ':!**/node_modules/**' \
  | xargs -0 node -e '
const fs = require("fs"), path = require("path");
// GitHub heading slugs: lowercase, drop punctuation, spaces to dashes, -N for repeats.
function slugs(file) {
  const seen = new Map(), out = new Set();
  let fence = false;
  for (const line of fs.readFileSync(file, "utf8").split("\n")) {
    if (/^\s*(```|~~~)/.test(line)) fence = !fence;
    const m = !fence && /^#{1,6}\s+(.*?)\s*#*\s*$/.exec(line);
    if (!m) continue;
    let s = m[1].replace(/<[^>]+>/g, "").replace(/\[([^\]]*)\]\([^)]*\)/g, "$1").toLowerCase()
      .replace(/[^\p{L}\p{N}\s_-]/gu, "").replace(/\s/g, "-");
    const n = seen.get(s) || 0;
    seen.set(s, n + 1);
    out.add(n ? `${s}-${n}` : s);
  }
  return out;
}
const cache = new Map();
const anchorsOf = (f) => cache.get(f) || (cache.set(f, slugs(f)), cache.get(f));
let bad = 0, checked = 0;
for (const file of process.argv.slice(1)) {
  const text = fs.readFileSync(file, "utf8").replace(/```[\s\S]*?```/g, "").replace(/`[^`\n]*`/g, "");
  const links = [...text.matchAll(/\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/g), ...text.matchAll(/(?:src|href)="([^"]+)"/g)].map((m) => m[1]);
  for (const link of links) {
    if (/^(https?:|mailto:|data:)/.test(link)) continue;
    checked++;
    const [target, anchor] = link.split("#");
    const resolved = target ? path.normalize(path.join(path.dirname(file), decodeURIComponent(target))) : file;
    if (!fs.existsSync(resolved)) { console.error(`${file}: broken link ${link}`); bad++; continue; }
    if (anchor && resolved.endsWith(".md") && !anchorsOf(resolved).has(decodeURIComponent(anchor).toLowerCase())) {
      console.error(`${file}: missing heading #${anchor} in ${resolved}`); bad++;
    }
  }
}
console.log(`links: ${checked} relative links checked, ${bad} broken`);
process.exit(bad ? 1 : 0);
'
