#!/usr/bin/env bash
# Tests for scripts/updater-manifest.py: which release file serves which
# system in latest.json, what is left out (with a warning) and what fails.
# Usage: scripts/tests/updater_manifest_test.sh   (or `make test-scripts`)
set -uo pipefail
script="$(cd "$(dirname "$0")/.." && pwd)/updater-manifest.py"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0
check() {
  if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; [ -f "$work/out" ] && sed 's/^/     /' "$work/out"; failures=$((failures + 1)); fi
}

# A `.sig` as the Tauri CLI writes it: base64 of minisign's signature file.
sig() {
  printf 'untrusted comment: signature from tauri secret key\nRUQ=\ntrusted comment: timestamp:1700000000\tfile:%s%s\nAAAA\n' "$1" "${2:+	version:$2}" | base64 | tr -d '\n'
}
# release <file[:sig-version|:nosig|:unbound]>...: a fresh dist-release.
release() {
  rm -rf "$work/dist" && mkdir -p "$work/dist"
  for spec in "$@"; do
    name="${spec%%:*}" how="${spec#*:}"; [ "$how" = "$spec" ] && how=9.9.9-beta.3
    : > "$work/dist/$name"
    case "$how" in
      nosig) ;;
      unbound) sig "$name" > "$work/dist/$name.sig" ;;
      *) sig "$name" "$how" > "$work/dist/$name.sig" ;;
    esac
  done
}
run() {
  rm -f "$work/latest.json"
  python3 -I "$script" --tag "${TAG:-v9.9.9-beta.3}" --dir "$work/dist" --out "$work/latest.json" \
    --notes "$work/notes.md" --pub-date 2026-11-04T00:00:00Z "$@" >"$work/out" 2>&1
}
# field <python expression on the manifest `m`>
field() { python3 -I -c 'import json,sys; m=json.load(open(sys.argv[1])); print(eval(sys.argv[2]))' "$work/latest.json" "$1"; }
printf '### Added\n\n- Update now\n' > "$work/notes.md"

mac=Contour_9.9.9-beta.3_universal.app.tar.gz
appimage=Contour_9.9.9-beta.3_amd64.AppImage
msi=Contour_9.9.9.103_x64_en-US.msi
release "$mac" "$appimage" "$msi" Contour_9.9.9-beta.3_amd64.deb:nosig Contour_9.9.9-beta.3_universal.dmg:nosig SHA256SUMS:nosig
run; rc=$?
check "every system gets its build" '[ $rc -eq 0 ] && [ "$(field "sorted(m[\"platforms\"])")" = "['"'"'darwin-aarch64'"'"', '"'"'darwin-x86_64'"'"', '"'"'linux-x86_64'"'"', '"'"'windows-x86_64'"'"']" ]'
check "both Macs get the universal bundle" '[ "$(field "m[\"platforms\"][\"darwin-aarch64\"][\"url\"]")" = "https://github.com/quangphung218/contour-design/releases/download/v9.9.9-beta.3/$mac" ] && [ "$(field "m[\"platforms\"][\"darwin-x86_64\"] == m[\"platforms\"][\"darwin-aarch64\"]")" = True ]'
check "Linux gets the AppImage, not the .deb" '[ "$(field "m[\"platforms\"][\"linux-x86_64\"][\"url\"].rsplit(\"/\", 1)[1]")" = "$appimage" ]'
check "Windows gets the .msi" '[ "$(field "m[\"platforms\"][\"windows-x86_64\"][\"url\"].rsplit(\"/\", 1)[1]")" = "$msi" ]'
check "the signature is the .sig file as written" '[ "$(field "m[\"platforms\"][\"linux-x86_64\"][\"signature\"]")" = "$(cat "$work/dist/$appimage.sig")" ]'
check "version without v, notes and date" '[ "$(field "m[\"version\"]")" = 9.9.9-beta.3 ] && [ "$(field "m[\"notes\"]")" = "$(printf "### Added\n\n- Update now")" ] && [ "$(field "m[\"pub_date\"]")" = 2026-11-04T00:00:00Z ]'
check "no warning when every build is signed" '! grep -q "::warning::" "$work/out"'

release "$mac" "$appimage" "$msi:nosig"
run; rc=$?
check "an unsigned .msi: Windows left out with a warning" '[ $rc -eq 0 ] && [ "$(field "\"windows-x86_64\" in m[\"platforms\"]")" = False ] && grep -q "::warning::$msi has no .sig" "$work/out"'
release "$mac" "$appimage"
run; rc=$?
check "no .msi (the Windows bundle failed): left out with a warning" '[ $rc -eq 0 ] && [ "$(field "len(m[\"platforms\"])")" = 3 ] && grep -q "::warning::no \*.msi" "$work/out"'
release "$mac:unbound" "$appimage:unbound"
run; rc=$?
check "a signature without a version is accepted" '[ $rc -eq 0 ] && [ "$(field "len(m[\"platforms\"])")" = 3 ]'

release "$mac:nosig" "$appimage:nosig" "$msi:nosig"
run; rc=$?
check "nothing signed: fails, no latest.json" '[ $rc -ne 0 ] && [ ! -e "$work/latest.json" ] && grep -q "no signed updater bundle" "$work/out"'
release "$mac" "$appimage:9.9.9-beta.2"
run; rc=$?
check "a signature made for another version: fails" '[ $rc -ne 0 ] && [ ! -e "$work/latest.json" ] && grep -q "made for version 9.9.9-beta.2" "$work/out"'
release "$mac" Contour_old.app.tar.gz
run; rc=$?
check "two macOS bundles: fails" '[ $rc -ne 0 ] && grep -q "more than one \*.app.tar.gz" "$work/out"'
release "$mac"
echo 'not base64!' > "$work/dist/$mac.sig"
run; rc=$?
check "a .sig that is not a Tauri signature: fails" '[ $rc -ne 0 ] && grep -q "is not a Tauri signature" "$work/out"'
release "$mac"
TAG=latest run; rc=$?
check "a tag that is not v<semver>: fails" '[ $rc -ne 0 ] && grep -q "latest is not v<semver>" "$work/out"'
release "Contour 9.9.9.app.tar.gz"
run; rc=$?
check "file names are URL-encoded" '[ $rc -eq 0 ] && [ "$(field "m[\"platforms\"][\"darwin-x86_64\"][\"url\"].rsplit(\"/\", 1)[1]")" = Contour%209.9.9.app.tar.gz ]'

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
