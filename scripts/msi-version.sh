#!/usr/bin/env bash
# Prints the Windows Installer version for a release version. WiX only takes
# numbers (major.minor.patch.build, major and minor <= 255, the rest <= 65535),
# so `tauri build --bundles msi` fails on 0.2.0-beta.2. The build field keeps
# release order: alpha.N -> N, beta.N -> 100+N, rc.N -> 200+N, release -> 1000.
# Usage: scripts/msi-version.sh 0.2.0-beta.2   (a leading "v" is accepted)
set -euo pipefail
v="${1:?usage: scripts/msi-version.sh x.y.z[-alpha|beta|rc.N]}"
v="${v#v}"
re='^([0-9]+)\.([0-9]+)\.([0-9]+)(-(alpha|beta|rc)\.([0-9]+))?$'
[[ "$v" =~ $re ]] || { echo "msi-version: $v is not x.y.z or x.y.z-(alpha|beta|rc).N" >&2; exit 1; }
major=${BASH_REMATCH[1]} minor=${BASH_REMATCH[2]} patch=${BASH_REMATCH[3]}
stage=${BASH_REMATCH[5]} n=${BASH_REMATCH[6]}
if [ "$major" -gt 255 ] || [ "$minor" -gt 255 ] || [ "$patch" -gt 65535 ]; then
  echo "msi-version: $v is past the MSI limits (255.255.65535)" >&2; exit 1
fi
case "$stage" in
  "") build=1000 ;;
  *)
    [ "$n" -lt 100 ] || { echo "msi-version: $stage.$n must be below 100" >&2; exit 1; }
    case "$stage" in alpha) build=$n ;; beta) build=$((100 + n)) ;; rc) build=$((200 + n)) ;; esac ;;
esac
echo "$major.$minor.$patch.$build"
