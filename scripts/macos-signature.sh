#!/usr/bin/env bash
# Checks that every Contour.app inside the given .dmg, .app.tar.gz or .app
# has a sealed signature, ad-hoc or Developer ID: `codesign --verify --deep
# --strict` passes and `codesign -dv` shows sealed resources, not only the
# linker's signature on the executable. macOS cannot keep a privacy grant
# (Documents, App Management) for an app without one (QA-112).
# Usage: scripts/macos-signature.sh Contour.dmg Contour.app.tar.gz ...
set -uo pipefail
[ $# -gt 0 ] || { echo "usage: scripts/macos-signature.sh <file.dmg|file.app.tar.gz|file.app>..." >&2; exit 2; }
work="$(mktemp -d)"
mounts=()
cleanup() {
  for m in ${mounts[@]+"${mounts[@]}"}; do hdiutil detach -quiet "$m" >/dev/null 2>&1 || hdiutil detach -force -quiet "$m" >/dev/null 2>&1; done
  rm -rf "$work"
}
trap cleanup EXIT
failures=0
fail() { echo "::error::$1"; failures=$((failures + 1)); }

# check_app <app> <where>: one .app.
check_app() {
  local app="$1" where="$2" info
  if ! codesign --verify --deep --strict --verbose=2 "$app" > "$work/verify" 2>&1; then
    fail "$where: codesign --verify --deep --strict failed"
    sed 's/^/     /' "$work/verify"
    return
  fi
  info="$(codesign -dv --verbose=2 "$app" 2>&1)"
  if ! grep -q '^Sealed Resources version=' <<< "$info" || grep -q 'linker-signed' <<< "$info"; then
    fail "$where: the signature is the linker's only, its resources are not sealed"
    grep -E '^(Identifier|CodeDirectory|Sealed Resources|Info.plist|Signature)' <<< "$info" | sed 's/^/     /'
    return
  fi
  echo "ok   $where: $(grep -E '^Signature=' <<< "$info" || echo 'Signature=?') $(grep -E '^Sealed Resources' <<< "$info")"
}

# check_dir <dir> <where>: every .app at the top of dir; none is a failure.
check_dir() {
  local dir="$1" where="$2" found=0 app
  for app in "$dir"/*.app; do
    [ -d "$app" ] || continue
    found=1
    check_app "$app" "$where ▸ ${app##*/}"
  done
  [ "$found" = 1 ] || fail "$where: no .app inside"
}

n=0
for f in "$@"; do
  n=$((n + 1))
  case "$f" in
    *.dmg)
      [ -f "$f" ] || { fail "$f: not found"; continue; }
      mnt="$work/mnt$n"
      mkdir -p "$mnt"
      if ! hdiutil attach -nobrowse -readonly -noautoopen -mountpoint "$mnt" "$f" > "$work/attach" 2>&1; then
        fail "$f: hdiutil attach failed"
        sed 's/^/     /' "$work/attach"
        continue
      fi
      mounts+=("$mnt")
      check_dir "$mnt" "${f##*/}"
      ;;
    *.app.tar.gz)
      [ -f "$f" ] || { fail "$f: not found"; continue; }
      dir="$work/tar$n"
      mkdir -p "$dir"
      if ! tar -xzf "$f" -C "$dir"; then
        fail "$f: tar could not unpack it"
        continue
      fi
      check_dir "$dir" "${f##*/}"
      ;;
    *.app)
      [ -d "$f" ] || { fail "$f: not found"; continue; }
      check_app "$f" "${f##*/}"
      ;;
    *) fail "$f: not a .dmg, .app.tar.gz or .app" ;;
  esac
done
[ "$failures" -eq 0 ] || { echo "macos-signature: $failures failed"; exit 1; }
echo "macos-signature: all sealed"
