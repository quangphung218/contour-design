#!/usr/bin/env bash
# Release notes for one version, from CHANGELOG.md.
#
#   scripts/release-notes.sh 0.2.0
#     The CHANGELOG.md section of that version, without its "### Highlights"
#     block (those bullets repeat lines below them). release.yml uses it for
#     the draft release and the updater's latest.json.
#
#   scripts/release-notes.sh --body 0.2.0 [--verified "<line>"] [--assets <file>] [--repo <owner/name>]
#     The GitHub release body: Highlights, What's changed, Verified, Downloads.
#     Highlights are the bullets of the version's optional "### Highlights"
#     block; without that block there is no Highlights part (the first
#     bullets would only repeat What's changed).
#     --verified is the maintainer's line, e.g. a link to the native QA note;
#       without it the body says the build was not checked by hand.
#     --assets is a file with one asset name per line
#       (gh release view v0.2.0 --json assets -q '.assets[].name');
#       only the files listed get a row. Without it every row is printed.
#     --repo defaults to $GITHUB_REPOSITORY, then quangphung218/contour-design.
#
# A leading "v" on the version is accepted. Fails when the version has no notes.
set -euo pipefail

usage() { echo "usage: scripts/release-notes.sh [--body] x.y.z [--verified <line>] [--assets <file>] [--repo <owner/name>]" >&2; exit 2; }

body=0 v="" verified="" assets="" repo="${GITHUB_REPOSITORY:-quangphung218/contour-design}"
while [ $# -gt 0 ]; do
  case "$1" in
    --body) body=1 ;;
    --verified) [ $# -ge 2 ] || usage; verified="$2"; shift ;;
    --assets) [ $# -ge 2 ] || usage; assets="$2"; shift ;;
    --repo) [ $# -ge 2 ] || usage; repo="$2"; shift ;;
    -*) usage ;;
    *) [ -z "$v" ] || usage; v="$1" ;;
  esac
  shift
done
[ -n "$v" ] || usage
v="${v#v}"
if [ $body -eq 0 ] && { [ -n "$verified" ] || [ -n "$assets" ]; }; then
  echo "--verified and --assets need --body" >&2; exit 2
fi
if [ -n "$assets" ] && [ ! -r "$assets" ]; then
  echo "cannot read the asset list $assets" >&2; exit 1
fi

file="$(cd "$(dirname "$0")/.." && pwd)/CHANGELOG.md"
notes="$(awk -v v="$v" '
  index($0, "## [" v "]") == 1 { p=1; next }
  p && (/^## \[/ || /^\[[^]]+\]: /) { exit }
  p { print }' "$file")"
[ -n "$(printf '%s' "$notes" | tr -d '[:space:]')" ] || { echo "CHANGELOG.md has no notes for $v" >&2; exit 1; }

# The section without its Highlights block.
without="$(printf '%s\n' "$notes" | awk '
  $0 == "### Highlights" { skip=1; next }
  skip && /^### / { skip=0 }
  !skip { print }')"

if [ $body -eq 0 ]; then
  printf '%s\n' "$without"
  exit 0
fi

# bullets <heading>: the bullets under "### <heading>", a continuation line
# (indented) joined to its bullet.
bullets() {
  printf '%s\n' "$notes" | awk -v h="### $1" '
    $0 == h { p=1; next }
    p && /^### / { p=0; done=1 }
    done { next }
    p && /^- / { if (b != "") print b; b=$0; next }
    p && /^[ \t]+[^ \t]/ && b != "" { sub(/^[ \t]+/, ""); b=b " " $0; next }
    END { if (b != "") print b }'
}

highlights="$(bullets Highlights)"
changes="$(printf '%s\n' "$without" | awk 'NF { started=1 } started')"

tag="v$v"
url="https://github.com/$repo/releases/download/$tag"
row() { # row <system> <file> <how to install>
  if [ -n "$assets" ] && ! grep -qxF "$2" "$assets"; then return; fi
  printf '| %s | [`%s`](%s/%s) | %s |\n' "$1" "$2" "$url" "$2" "$3"
}

if [ -n "$highlights" ]; then
  printf '## Highlights\n\n%s\n\n' "$highlights"
fi
printf "## What's changed\n\n%s\n\n" "$changes"
printf '## Verified\n\n'
if [ -n "$verified" ]; then
  printf '%s\n\n' "$verified"
else
  printf 'Automated checks only (CI, the Linux and Windows smoke tests); not yet checked by hand.\n\n'
fi
rows="$(
  row "macOS 13.3 or later, Apple silicon and Intel" "Contour_${v}_universal.dmg" "Drag to Applications, then allow it once ([how](https://github.com/$repo#first-open-on-macos))"
  row "Ubuntu 22.04 or 24.04, x64" "Contour_${v}_amd64.deb" "\`sudo apt install ./Contour_${v}_amd64.deb\`"
  row "Ubuntu 22.04 or 24.04, x64" "Contour_${v}_amd64.AppImage" "Make it executable and run it"
  row "Windows 10 and 11, x64" "Contour_${v}_x64_en-US.msi" "SmartScreen asks once: More info ▸ Run anyway"
)"
printf '## Downloads\n\n'
if [ -n "$rows" ]; then
  printf '| System | File | Install |\n|---|---|---|\n%s\n\n' "$rows"
fi
printf '%s\n' "\`SHA256SUMS\` lists a checksum for every file: \`shasum -a 256 --ignore-missing -c SHA256SUMS\` on macOS, \`sha256sum --ignore-missing -c SHA256SUMS\` on Linux. The macOS app is signed ad-hoc, without an Apple Developer ID: \`codesign --verify --deep --strict /Applications/Contour.app\` should say \"valid on disk\" and \"satisfies its Designated Requirement\". The \`.app.tar.gz\`, \`.sig\` and \`latest.json\` files are for Update now in the app."
