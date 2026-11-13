#!/usr/bin/env bash
# Tests for .github/workflows/release.yml and windows-smoke.yml: runs the
# shell of their steps with fake `gh` and `npx`, checks where the signing
# secrets can go and which jobs wait for which.
# Needs ruby (YAML parsing; present on macOS and GitHub's Ubuntu runners).
# Usage: scripts/tests/release_workflow_test.sh   (or `make test-scripts`)
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
workflow="$here/../../.github/workflows/release.yml"
windows_smoke="$here/../../.github/workflows/windows-smoke.yml"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0
check() {
  if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; [ -f "$work/out" ] && sed 's/^/     /' "$work/out"; failures=$((failures + 1)); fi
}

# step <[job/]name> <file> [workflow]: the `run:` script of the step with
# that name, in that job if one is given (in release.yml unless another
# workflow is given).
step() {
  ruby -ryaml -e '
    wf = YAML.load_file(ARGV[0])
    job, name = ARGV[1].include?("/") ? ARGV[1].split("/", 2) : [nil, ARGV[1]]
    jobs = job ? [wf["jobs"].fetch(job)] : wf["jobs"].values
    s = jobs.flat_map { |j| j["steps"] || [] }.find { |x| x["name"] == name }
    abort("no step #{ARGV[1]}") unless s
    print s["run"].gsub(/\$\{\{\s*matrix\.args\s*\}\}/, "--bundles app")' "${3:-$workflow}" "$1" > "$2"
}

# Secrets: only in the env of the "Tauri bundle" step, never via GITHUB_ENV.
ruby -ryaml -e '
  wf = YAML.load_file(ARGV[0])
  wf["jobs"].each do |job, j|
    (j["steps"] || []).each do |s|
      text = s.to_yaml
      next unless text.include?("secrets.")
      kinds = text.scan(/secrets\.([A-Z_]+)/).flatten.map { |n| n.start_with?("APPLE_") ? "apple" : n.start_with?("TAURI_SIGNING_") ? "updater" : n }.uniq.sort
      puts "#{job}/#{s["name"]}: #{kinds.join(",")}"
    end
  end' "$workflow" > "$work/secret-steps"
# The Linux and Windows bundles never see the Apple ones: they are not in that job.
check "Apple secrets only in the macOS Tauri bundle step, the updater key in each Tauri bundle step" '[ "$(cat "$work/secret-steps")" = "$(printf "bundle/Tauri bundle: updater\nmacos-universal/Tauri bundle: apple,updater")" ]'
check "nothing writes to GITHUB_ENV" '! grep -q GITHUB_ENV "$workflow"'

# Signing env reaching the Tauri CLI (fake npx records APPLE_* it sees).
mkdir -p "$work/bin"
cat > "$work/bin/npx" <<'SH'
#!/bin/sh
env | grep '^APPLE_' | sort > "$WORK/npx-env"
env | grep '^TAURI_SIGNING_' | sort > "$WORK/npx-updater-env"
printf '%s\n' "$@" > "$WORK/npx-args"
SH
chmod +x "$work/bin/npx"
step "macos-universal/Tauri bundle" "$work/macos-bundle.sh"
step "bundle/Tauri bundle" "$work/bundle.sh"
bundle() { (cd "$work" && env WORK="$work" RUNNER_TEMP="$work" PATH="$work/bin:$PATH" TAURI_CLI_VERSION=0 "$@" bash "$work/macos-bundle.sh") >"$work/out" 2>&1; }
empty="APPLE_CERTIFICATE= APPLE_CERTIFICATE_PASSWORD= APPLE_SIGNING_IDENTITY= APPLE_ID= APPLE_PASSWORD= APPLE_TEAM_ID="
bundle RUNNER_OS=macOS $empty
check "no secrets: unsigned build, warning, no empty APPLE_* passed on" '[ ! -s "$work/npx-env" ] && grep -q "::warning::macOS signing secrets are not set" "$work/out"'
bundle RUNNER_OS=macOS $empty APPLE_CERTIFICATE=c APPLE_CERTIFICATE_PASSWORD=p APPLE_SIGNING_IDENTITY=i
check "signing only: certificate passed on, notarisation unset" '[ "$(cut -d= -f1 "$work/npx-env" | LC_ALL=C sort | tr "\n" " ")" = "APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY " ] && grep -q "not notarised" "$work/out" && ! grep -qE "=(c|p|i)$" "$work/out"'
bundle RUNNER_OS=macOS $empty APPLE_CERTIFICATE=c APPLE_CERTIFICATE_PASSWORD=p APPLE_SIGNING_IDENTITY=i APPLE_ID=a APPLE_PASSWORD=w APPLE_TEAM_ID=t TAURI_SIGNING_PRIVATE_KEY=k3y
check "signing and notarising: all six passed on" '[ "$(wc -l < "$work/npx-env" | tr -d " ")" = 6 ] && ! grep -q "::warning::" "$work/out"'

# MSI version: the step runs in apps/desktop and calls ../../scripts/msi-version.sh.
mkdir -p "$work/apps/desktop" && ln -s "$here/.." "$work/scripts"
# wix_version: bundle.windows.wix.version from the JSON after --config, or "none".
wix_version() {
  ruby -rjson -e 'a = File.readlines(ARGV[0], chomp: true); i = a.index("--config")
    puts(i ? JSON.parse(a[i + 1]).dig("bundle", "windows", "wix", "version").to_s : "none")' "$work/npx-args"
}
desktop() { (cd "$work/apps/desktop" && env WORK="$work" PATH="$work/bin:$PATH" TAURI_CLI_VERSION=0 "$@" bash "$work/bundle.sh") >"$work/out" 2>&1; }
desktop RUNNER_OS=Windows TAG=v9.9.9-beta.2 $empty; rc=$?
check "Windows gets a numeric MSI version through --config" '[ $rc -eq 0 ] && [ "$(wix_version)" = 9.9.9.102 ]'
for os in macOS Linux; do
  desktop RUNNER_OS=$os TAG=v9.9.9-beta.2 $empty
  check "$os gets no --config" '[ "$(wix_version)" = none ]'
done

# The updater key: with it the bundles get updater artifacts (signed by
# the CLI); without it they build anyway, with a warning, and the empty
# variables are not passed on. Never printed.
updater_config() { grep -c '"createUpdaterArtifacts":true' "$work/npx-args"; }
nokey="TAURI_SIGNING_PRIVATE_KEY= TAURI_SIGNING_PRIVATE_KEY_PASSWORD="
withkey="TAURI_SIGNING_PRIVATE_KEY=k3y TAURI_SIGNING_PRIVATE_KEY_PASSWORD=pw"
bundle RUNNER_OS=macOS $empty $nokey; rc=$?
check "macOS without the updater key: bundle anyway, warning, nothing passed on" '[ $rc -eq 0 ] && [ "$(updater_config)" = 0 ] && [ ! -s "$work/npx-updater-env" ] && grep -q "::warning::TAURI_SIGNING_PRIVATE_KEY is not set" "$work/out"'
bundle RUNNER_OS=macOS $empty $withkey; rc=$?
check "macOS with the updater key: updater artifacts, key passed on, not printed" '[ $rc -eq 0 ] && [ "$(updater_config)" = 1 ] && [ "$(cut -d= -f1 "$work/npx-updater-env" | LC_ALL=C sort | tr "\n" " ")" = "TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD " ] && ! grep -qE "k3y|=pw" "$work/out" && ! grep -q "::warning::TAURI" "$work/out"'
for os in Linux Windows; do
  desktop RUNNER_OS=$os TAG=v9.9.9-beta.2 $empty $nokey; rc=$?
  check "$os without the updater key: bundle anyway, warning" '[ $rc -eq 0 ] && [ "$(updater_config)" = 0 ] && [ ! -s "$work/npx-updater-env" ] && grep -q "::warning::TAURI_SIGNING_PRIVATE_KEY is not set" "$work/out"'
  desktop RUNNER_OS=$os TAG=v9.9.9-beta.2 $empty $withkey; rc=$?
  check "$os with the updater key: updater artifacts" '[ $rc -eq 0 ] && [ "$(updater_config)" = 1 ] && [ -s "$work/npx-updater-env" ]'
done
check "Windows with the updater key keeps its MSI version" '[ "$(wix_version)" = 9.9.9.102 ]'

# The bundles' files: the updater bundles and their .sig are collected,
# the macOS one named like the .dmg.
step "macos-universal/Collect files" "$work/mac-collect.sh"
rm -rf "$work/mc" && mkdir -p "$work/mc/target/universal-apple-darwin/release/bundle/macos" "$work/mc/target/universal-apple-darwin/release/bundle/dmg"
touch "$work/mc/target/universal-apple-darwin/release/bundle/dmg/Contour_9.9.9_universal.dmg" \
  "$work/mc/target/universal-apple-darwin/release/bundle/macos/Contour.app.tar.gz" \
  "$work/mc/target/universal-apple-darwin/release/bundle/macos/Contour.app.tar.gz.sig"
(cd "$work/mc" && env TAG=v9.9.9 bash "$work/mac-collect.sh") >"$work/out" 2>&1; rc=$?
check "macOS: the .dmg, Contour_<v>_universal.app.tar.gz and its .sig" '[ $rc -eq 0 ] && [ "$(ls "$work/mc/dist-release" | tr "\n" " ")" = "Contour_9.9.9_universal.app.tar.gz Contour_9.9.9_universal.app.tar.gz.sig Contour_9.9.9_universal.dmg " ]'
step "bundle/Collect files" "$work/collect.sh"
rm -rf "$work/lc" && mkdir -p "$work/lc/out/appimage" "$work/lc/out/deb"
touch "$work/lc/out/appimage/C.AppImage" "$work/lc/out/appimage/C.AppImage.sig" "$work/lc/out/deb/C.deb" "$work/lc/out/appimage/build_appimage.sh"
sed -i.bak 's|\${{ matrix.out }}|out|' "$work/collect.sh"
(cd "$work/lc" && bash "$work/collect.sh") >"$work/out" 2>&1; rc=$?
check "Linux: the AppImage, its .sig and the .deb" '[ $rc -eq 0 ] && [ "$(ls "$work/lc/dist-release" | tr "\n" " ")" = "C.AppImage C.AppImage.sig C.deb " ]'

# The draft's files: latest.json from the .sig files, in SHA256SUMS.
step "Checksums and notes" "$work/sums.sh"
printf '## [Unreleased]\n\n## [9.9.9] - 2026-11-04\n\n### Added\n\n- Update now\n' > "$work/CHANGELOG.md"
sums() {
  rm -rf "$work/dist-release" && mkdir -p "$work/dist-release"
  for f in "$@"; do touch "$work/dist-release/$f"; done
  (cd "$work" && env TAG=v9.9.9 bash "$work/sums.sh") >"$work/out" 2>&1
}
signature="$(printf 'untrusted comment: x\nRUQ=\ntrusted comment: timestamp:1\tfile:a\tversion:9.9.9\nAAAA\n' | base64 | tr -d '\n')"
sums Contour_9.9.9_universal.dmg Contour_9.9.9_universal.app.tar.gz Contour_9.9.9_amd64.AppImage Contour_9.9.9_amd64.deb; rc=$?
check "no .sig: no latest.json, a warning, checksums as before" '[ $rc -eq 0 ] && [ ! -e "$work/dist-release/latest.json" ] && grep -q "::warning::no updater signatures" "$work/out" && [ "$(wc -l < "$work/dist-release/SHA256SUMS" | tr -d " ")" = 4 ]'
sums Contour_9.9.9_universal.dmg Contour_9.9.9_universal.app.tar.gz Contour_9.9.9_amd64.AppImage Contour_9.9.9_amd64.deb
printf '%s' "$signature" > "$work/dist-release/Contour_9.9.9_universal.app.tar.gz.sig"
printf '%s' "$signature" > "$work/dist-release/Contour_9.9.9_amd64.AppImage.sig"
(cd "$work" && env TAG=v9.9.9 bash "$work/sums.sh") >"$work/out" 2>&1; rc=$?
check "signed bundles: latest.json for them, in SHA256SUMS with the .sig files" '[ $rc -eq 0 ] && grep -q "\"darwin-aarch64\"" "$work/dist-release/latest.json" && grep -q "\"linux-x86_64\"" "$work/dist-release/latest.json" && grep -q " latest.json$" "$work/dist-release/SHA256SUMS" && grep -q " Contour_9.9.9_amd64.AppImage.sig$" "$work/dist-release/SHA256SUMS" && grep -q "Update now" "$work/dist-release/latest.json"'
printf '%s' "$(printf 'untrusted comment: x\nRUQ=\ntrusted comment: timestamp:1\tfile:a\tversion:9.9.8\nAAAA\n' | base64 | tr -d '\n')" > "$work/dist-release/Contour_9.9.9_amd64.AppImage.sig"
rm -f "$work/dist-release/latest.json" "$work/dist-release/SHA256SUMS"
(cd "$work" && env TAG=v9.9.9 bash -e "$work/sums.sh") >"$work/out" 2>&1; rc=$?
check "a .sig of another version stops the draft" '[ $rc -ne 0 ] && [ ! -e "$work/dist-release/latest.json" ]'

# Draft release: a published release is never rewritten.
cat > "$work/bin/gh" <<'SH'
#!/bin/sh
echo "gh $*" >> "$WORK/gh-calls"
case "$1 $2" in
  "release view") [ -n "$GH_STATE" ] || exit 1; [ "$GH_STATE" = draft ] && echo true || echo false ;;
esac
SH
chmod +x "$work/bin/gh"
step "Create the draft release" "$work/release.sh"
mkdir -p "$work/dist-release" && touch "$work/dist-release/a.dmg" "$work/release-notes.md"
release() { rm -f "$work/gh-calls"; (cd "$work" && env WORK="$work" PATH="$work/bin:$PATH" TAG=v9.9.9 GH_STATE="$1" bash "$work/release.sh") >"$work/out" 2>&1; }
release published; rc=$?
check "a published release is refused, nothing uploaded" '[ $rc -ne 0 ] && ! grep -q "release upload" "$work/gh-calls"'
release draft; rc=$?
check "a draft gets its files replaced" '[ $rc -eq 0 ] && grep -q "release upload v9.9.9 .*--clobber" "$work/gh-calls"'
check "a draft gets the current CHANGELOG notes" 'grep -qx "gh release edit v9.9.9 --notes-file release-notes.md" "$work/gh-calls"'
release published; rc=$?
check "a published release keeps its notes" '! grep -q "release edit" "$work/gh-calls"'
release ""; rc=$?
check "no release yet: a draft is created" '[ $rc -eq 0 ] && grep -q "release create v9.9.9 .*--draft" "$work/gh-calls"'

# The bundles start without waiting for any tests; the draft release waits
# for the macOS tests (below) and never for the Windows build.
ruby -ryaml -e '
  jobs = YAML.load_file(ARGV[0])["jobs"]
  call = ->(name) { j = jobs[name] or abort("no job #{name}"); "#{j["uses"]} only=#{(j["with"] || {})["only"]}" }
  puts "bundle needs: #{Array(jobs["bundle"]["needs"]).sort.join(",")}"
  puts "macos-binary needs: #{Array(jobs["macos-binary"]["needs"]).sort.join(",")}"
  puts "ui needs: #{Array(jobs["ui"]["needs"]).sort.join(",")}"
  puts "macos-tests: #{call.("macos-tests")}"
  puts "windows-build: #{call.("windows-build")}"
  waiting = jobs.select { |_, j| Array(j["needs"]).include?("windows-build") }.keys
  puts "waits for windows: #{waiting.join(",")}"' "$workflow" > "$work/gates" 2>&1
check "bundles start after the version check, not after the tests" 'grep -qx "bundle needs: check,ui" "$work/gates" && grep -qx "macos-binary needs: check" "$work/gates" && grep -qx "ui needs: check" "$work/gates"'
check "macOS tests come from ci.yml with only=macos" 'grep -qx "macos-tests: ./.github/workflows/ci.yml only=macos" "$work/gates"'
check "the Windows build runs from ci.yml with only=windows" 'grep -qx "windows-build: ./.github/workflows/ci.yml only=windows" "$work/gates"'
check "no job waits for the Windows build" 'grep -qx "waits for windows: " "$work/gates"'

# The draft release waits for the Linux smoke test of the bundle just built.
ruby -ryaml -e '
  jobs = YAML.load_file(ARGV[0])["jobs"]
  smoke = jobs["linux-smoke"] or abort("no job linux-smoke")
  names = jobs["bundle"]["strategy"]["matrix"]["include"].map { |m| m["name"] }
  upload = jobs["bundle"]["steps"].find { |s| s["uses"].to_s.start_with?("actions/upload-artifact") }
  built = names.map { |n| upload["with"]["name"].sub("${{ matrix.name }}", n) }
  puts "release needs: #{Array(jobs["release"]["needs"]).sort.join(",")}"
  puts "smoke: #{smoke["uses"]} needs=#{Array(smoke["needs"]).join(",")}"
  puts "smoke artifact built: #{built.include?(smoke["with"]["artifact"])}"' "$workflow" > "$work/smoke" 2>&1
check "the draft release waits for the Linux smoke test" 'grep -q "release needs: .*linux-smoke" "$work/smoke"'
check "the Linux smoke test runs linux-smoke.yml after the bundles" 'grep -qx "smoke: ./.github/workflows/linux-smoke.yml needs=bundle" "$work/smoke"'
check "the Linux smoke test gets an artifact the bundle job uploads" 'grep -qx "smoke artifact built: true" "$work/smoke"'

# The draft release also waits for the Windows smoke test of the .msi, which
# is skipped (not failed) when the best-effort Windows bundle did not build.
ruby -ryaml -e '
  jobs = YAML.load_file(ARGV[0])["jobs"]
  smoke = jobs["windows-smoke"] or abort("no job windows-smoke")
  windows = jobs["bundle"]["strategy"]["matrix"]["include"].find { |m| m["name"] == "windows-x64" }
  upload = jobs["bundle"]["steps"].find { |s| s["uses"].to_s.start_with?("actions/upload-artifact") }
  puts "smoke: #{smoke["uses"]} needs=#{Array(smoke["needs"]).join(",")}"
  puts "smoke artifact: #{smoke["with"]["artifact"] == upload["with"]["name"].sub("${{ matrix.name }}", "windows-x64")}"
  puts "windows bundle best effort: #{windows["best_effort"]} continue-on-error: #{jobs["bundle"]["continue-on-error"]}"' "$workflow" > "$work/wsmoke" 2>&1
check "the draft release waits for the macOS tests, every bundle and both smoke tests" 'grep -qx "release needs: bundle,linux-smoke,macos-tests,macos-universal,windows-smoke" "$work/smoke"'
check "the Windows smoke test runs windows-smoke.yml after the bundles" 'grep -qx "smoke: ./.github/workflows/windows-smoke.yml needs=bundle" "$work/wsmoke"'
check "the Windows smoke test gets the .msi artifact the bundle job uploads" 'grep -qx "smoke artifact: true" "$work/wsmoke"'
check "the Windows bundle stays best effort" 'grep -qx "windows bundle best effort: true continue-on-error: \${{ matrix.best_effort }}" "$work/wsmoke"'

# The macOS universal bundle: two arch binaries and the shared UI, joined in
# one job; the draft collects every bundle-* artifact and nothing else.
ruby -ryaml -rjson -e '
  jobs = YAML.load_file(ARGV[0])["jobs"]
  dl = jobs["release"]["steps"].find { |s| s["uses"].to_s.start_with?("actions/download-artifact") }
  puts "draft downloads: #{dl["with"]["pattern"]}"
  puts "macos-universal needs: #{Array(jobs["macos-universal"]["needs"]).sort.join(",")}"
  puts "macos-binary targets: #{jobs["macos-binary"]["strategy"]["matrix"]["target"].sort.join(",")}"
  steps = jobs["bundle"]["steps"]
  get = steps.index { |s| s["uses"].to_s.start_with?("actions/download-artifact") && s.dig("with", "name") == "ui-dist" }
  present = steps.index { |s| s["name"] == "UI bundle present" }
  tauri = steps.index { |s| s["name"] == "Tauri bundle" }
  ok = get && present && tauri && get < present && present < tauri && steps[present]["run"].include?("crates/od-ui/dist/index.html")
  puts "bundle checks the UI before Tauri: #{ok ? true : false}"
  dist = JSON.parse(File.read(ARGV[1])).dig("build", "frontendDist").to_s
  puts "frontendDist is the UI: #{File.expand_path(dist, File.dirname(ARGV[1])) == File.expand_path(ARGV[2])}"
  caches = jobs.values.flat_map { |j| j["steps"] || [] }.select { |s| s["uses"].to_s.start_with?("Swatinem/rust-cache") }
  puts "caches saved on manual runs only: #{!caches.empty? && caches.all? { |s| s.dig("with", "save-if") == "${{ github.event_name == \x27workflow_dispatch\x27 }}" }}"
' "$workflow" "$here/../../apps/desktop/tauri.conf.json" "$here/../../crates/od-ui/dist" > "$work/mac" 2>&1
check "the draft release collects only bundle-* artifacts" 'grep -qx "draft downloads: bundle-\*" "$work/mac"'
check "the universal bundle waits for both macOS binaries and the UI" 'grep -qx "macos-universal needs: macos-binary,ui" "$work/mac"'
check "macOS binaries are built for Apple silicon and Intel" 'grep -qx "macos-binary targets: aarch64-apple-darwin,x86_64-apple-darwin" "$work/mac"'
check "the Linux and Windows bundles check the downloaded UI before Tauri" 'grep -qx "bundle checks the UI before Tauri: true" "$work/mac"'
# The macOS binaries build against an empty UI placeholder (release.yml).
check "frontendDist (compiled in) is not the UI resource folder" 'grep -qx "frontendDist is the UI: false" "$work/mac"'
check "Rust caches are saved on manual runs only, not by tags" 'grep -qx "caches saved on manual runs only: true" "$work/mac"'

# windows-smoke.yml: the smoke jobs run only when "find the installer" found
# one, and that step decides from what the bundle job left.
ruby -ryaml -e '
  jobs = YAML.load_file(ARGV[0])["jobs"]
  installer = jobs["installer"] or abort("no job installer")
  dl = installer["steps"].find { |s| s["uses"].to_s.start_with?("actions/download-artifact") }
  puts "installer needs: #{Array(installer["needs"]).join(",")} download continue-on-error: #{dl["continue-on-error"]}"
  puts "installer output: #{installer["outputs"]["msi"]}"
  puts "smoke needs: #{Array(jobs["smoke"]["needs"]).join(",")} if: #{jobs["smoke"]["if"]}"
  puts "smoke runners: #{jobs["smoke"]["strategy"]["matrix"]["os"].join(",")}"' "$windows_smoke" > "$work/wjobs" 2>&1
check "a missing artifact does not fail the download in the installer job" 'grep -qx "installer needs: bundle download continue-on-error: true" "$work/wjobs"'
check "the smoke jobs run only when the installer job found an .msi" 'grep -qx "installer output: \${{ steps.find.outputs.msi }}" "$work/wjobs" && grep -qx "smoke needs: installer if: \${{ !cancelled() && needs.installer.outputs.msi == '"'"'true'"'"' }}" "$work/wjobs"'
check "the smoke test runs on Windows Server 2025 and 2022" 'grep -qx "smoke runners: windows-2025,windows-2022" "$work/wjobs"'

# Fetching the old .msi counts as a download: pull requests skip the upgrade.
ruby -ryaml -e '
  steps = YAML.load_file(ARGV[0])["jobs"]["smoke"]["steps"]
  ifs = steps.select { |s| s["run"].to_s.include?("gh release download") }.map { |s| s["if"] }
  puts "downloads: #{ifs.length} if: #{ifs.join("|")}"' "$windows_smoke" > "$work/wupgrade" 2>&1
check "pull requests do not download the previous release" 'grep -qx "downloads: 1 if: \${{ !cancelled() && github.event_name != '"'"'pull_request'"'"' }}" "$work/wupgrade"'

step "Find the installer" "$work/find.sh" "$windows_smoke"
find_installer() {
  rm -rf "$work/find" && mkdir -p "$work/find/dist-release" && : > "$work/find/output"
  for f in "${@:2}"; do touch "$work/find/dist-release/$f"; done
  (cd "$work/find" && env GITHUB_OUTPUT="$work/find/output" FROM_RELEASE="$1" bash "$work/find.sh") >"$work/out" 2>&1
}
find_installer 1 Contour_9.9.9_x64_en-US.msi; rc=$?
check "release with an .msi: the smoke test runs" '[ $rc -eq 0 ] && grep -qx "msi=true" "$work/find/output"'
find_installer 1; rc=$?
check "release without an .msi: skipped with a warning, the draft goes on" '[ $rc -eq 0 ] && grep -qx "msi=false" "$work/find/output" && grep -q "::warning::no Windows installer" "$work/out"'
find_installer 0; rc=$?
check "built on the branch without an .msi: fails" '[ $rc -ne 0 ] && ! grep -q "msi=" "$work/find/output"'
find_installer 1 a.msi b.msi; rc=$?
check "two .msi files: fails" '[ $rc -ne 0 ] && ! grep -q "msi=" "$work/find/output"'

# The branch build and the smoke test agree with release.yml on the MSI version.
printf '[workspace.package]\nversion = "9.9.9-rc.3"\n' > "$work/Cargo.toml"
step "Tauri bundle" "$work/wbundle.sh" "$windows_smoke"
(cd "$work/apps/desktop" && env WORK="$work" PATH="$work/bin:$PATH" TAURI_CLI_VERSION=0 bash "$work/wbundle.sh") >"$work/out" 2>&1; rc=$?
check "the branch's .msi gets the numeric version from Cargo.toml" '[ $rc -eq 0 ] && [ "$(wix_version)" = 9.9.9.203 ] && grep -qx -- "--bundles" "$work/npx-args" && grep -qx msi "$work/npx-args"'
step "Installer version" "$work/wversion.sh" "$windows_smoke"
: > "$work/version-output"
(cd "$work" && env GITHUB_OUTPUT="$work/version-output" bash "$work/wversion.sh") >"$work/out" 2>&1; rc=$?
check "the smoke test expects that same installer version" '[ $rc -eq 0 ] && grep -qx "msi=9.9.9.203" "$work/version-output"'

# The upgrade starts from the latest published release that is not the one
# under test (fake gh: RELEASES is `gh release list --json` output, MSI_TAGS
# the tags with an .msi; GH_FAILS=list|download makes that call fail).
cat > "$work/bin/gh" <<'SH'
#!/bin/sh
[ "$2" = "${GH_FAILS:-}" ] && { echo "HTTP 502" >&2; exit 1; }
jq_arg() { while [ $# -gt 0 ]; do [ "$1" = --jq ] && { printf '%s' "$2"; return; }; shift; done; }
case "$1 $2" in
  "release list") printf '%s' "$RELEASES" | jq -r "$(jq_arg "$@")" ;;
  "release view")
    case " $MSI_TAGS " in
      *" $3 "*) assets="[{\"name\":\"Contour_$3_x64_en-US.msi\"},{\"name\":\"SHA256SUMS\"}]" ;;
      *) assets='[{"name":"SHA256SUMS"}]' ;;
    esac
    printf '{"assets":%s}' "$assets" | jq -r "$(jq_arg "$@")" ;;
  "release download")
    case " $MSI_TAGS " in *" $3 "*) mkdir -p previous && touch "previous/Contour_$3_x64_en-US.msi" ;; *) echo "no assets match" >&2; exit 1 ;; esac ;;
esac
SH
chmod +x "$work/bin/gh"
step "Previous release" "$work/previous.sh" "$windows_smoke"
previous() {
  rm -rf "$work/prev" && mkdir -p "$work/prev" && : > "$work/prev/output"
  (cd "$work/prev" && env GITHUB_OUTPUT="$work/prev/output" GITHUB_REPOSITORY=o/r VERSION=9.9.9-rc.3 PATH="$work/bin:$PATH" \
    RELEASES="$1" MSI_TAGS="$2" GH_FAILS="${3:-}" bash "$work/previous.sh") >"$work/out" 2>&1
}
both='[{"tagName":"v9.9.9-rc.2","publishedAt":"2026-01-01T00:00:00Z"},{"tagName":"v9.9.9-rc.3","publishedAt":"2026-01-02T00:00:00Z"},{"tagName":"v9.9.9-rc.1","publishedAt":"2025-12-01T00:00:00Z"}]'
previous "$both" "v9.9.9-rc.1 v9.9.9-rc.2 v9.9.9-rc.3"; rc=$?
check "upgrade from the latest release other than the one under test" '[ $rc -eq 0 ] && grep -qx "msi=previous/Contour_v9.9.9-rc.2_x64_en-US.msi" "$work/prev/output"'
previous '[{"tagName":"v9.9.9-rc.3","publishedAt":"2026-01-02T00:00:00Z"}]' "v9.9.9-rc.3"; rc=$?
check "no other release: the upgrade is skipped with a warning" '[ $rc -eq 0 ] && [ ! -s "$work/prev/output" ] && grep -q "::warning::no published release other than v9.9.9-rc.3" "$work/out"'
previous "$both" ""; rc=$?
check "no .msi in that release: the upgrade is skipped with a warning" '[ $rc -eq 0 ] && [ ! -s "$work/prev/output" ] && grep -q "::warning::no .msi in v9.9.9-rc.2" "$work/out"'
previous "$both" "v9.9.9-rc.2" download; rc=$?
check "a failed download fails the step" '[ $rc -ne 0 ] && [ ! -s "$work/prev/output" ] && grep -q "::error::downloading the .msi of v9.9.9-rc.2 failed" "$work/out"'
previous "$both" "v9.9.9-rc.2" view; rc=$?
check "a failed asset lookup fails the step" '[ $rc -ne 0 ] && [ ! -s "$work/prev/output" ] && grep -q "::error::reading the assets of v9.9.9-rc.2 failed" "$work/out"'
previous "$both" "v9.9.9-rc.2" list; rc=$?
check "a failed release list fails the step" '[ $rc -ne 0 ] && [ ! -s "$work/prev/output" ] && grep -q "::error::listing the releases failed" "$work/out"'

[ "$failures" -eq 0 ] && echo "release.yml: all checks passed" || { echo "release.yml: $failures failed"; exit 1; }
