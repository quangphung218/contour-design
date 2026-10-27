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

# step <name> <file> [workflow]: the `run:` script of the step with that
# name (in release.yml unless another workflow is given).
step() {
  ruby -ryaml -e '
    wf = YAML.load_file(ARGV[0])
    s = wf["jobs"].values.flat_map { |j| j["steps"] || [] }.find { |x| x["name"] == ARGV[1] }
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
      puts "#{job}/#{s["name"]}"
    end
  end' "$workflow" > "$work/secret-steps"
check "signing secrets appear only in the Tauri bundle step" '[ "$(cat "$work/secret-steps")" = "bundle/Tauri bundle" ]'
check "nothing writes to GITHUB_ENV" '! grep -q GITHUB_ENV "$workflow"'

# Signing env reaching the Tauri CLI (fake npx records APPLE_* it sees).
mkdir -p "$work/bin"
cat > "$work/bin/npx" <<'SH'
#!/bin/sh
env | grep '^APPLE_' | sort > "$WORK/npx-env"
printf '%s\n' "$@" > "$WORK/npx-args"
SH
chmod +x "$work/bin/npx"
step "Tauri bundle" "$work/bundle.sh"
bundle() { (cd "$work" && env WORK="$work" PATH="$work/bin:$PATH" TAURI_CLI_VERSION=0 "$@" bash "$work/bundle.sh") >"$work/out" 2>&1; }
empty="APPLE_CERTIFICATE= APPLE_CERTIFICATE_PASSWORD= APPLE_SIGNING_IDENTITY= APPLE_ID= APPLE_PASSWORD= APPLE_TEAM_ID="
bundle RUNNER_OS=macOS $empty
check "no secrets: unsigned build, warning, no empty APPLE_* passed on" '[ ! -s "$work/npx-env" ] && grep -q "::warning::macOS signing secrets are not set" "$work/out"'
bundle RUNNER_OS=macOS $empty APPLE_CERTIFICATE=c APPLE_CERTIFICATE_PASSWORD=p APPLE_SIGNING_IDENTITY=i
check "signing only: certificate passed on, notarisation unset" '[ "$(cut -d= -f1 "$work/npx-env" | LC_ALL=C sort | tr "\n" " ")" = "APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY " ] && grep -q "not notarised" "$work/out" && ! grep -qE "=(c|p|i)$" "$work/out"'
bundle RUNNER_OS=Linux $empty
check "other runners get no APPLE_* at all" '[ ! -s "$work/npx-env" ]'

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

# Tests before bundles: macOS gates them, the Windows build never does.
ruby -ryaml -e '
  jobs = YAML.load_file(ARGV[0])["jobs"]
  call = ->(name) { j = jobs[name] or abort("no job #{name}"); "#{j["uses"]} only=#{(j["with"] || {})["only"]}" }
  puts "bundle needs: #{Array(jobs["bundle"]["needs"]).sort.join(",")}"
  puts "macos-tests: #{call.("macos-tests")}"
  puts "windows-build: #{call.("windows-build")}"
  waiting = jobs.select { |_, j| Array(j["needs"]).include?("windows-build") }.keys
  puts "waits for windows: #{waiting.join(",")}"' "$workflow" > "$work/gates" 2>&1
check "bundles wait for the macOS tests, not for Windows" 'grep -qx "bundle needs: check,macos-tests" "$work/gates"'
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
check "the draft release waits for the Windows smoke test" 'grep -qx "release needs: bundle,linux-smoke,windows-smoke" "$work/smoke"'
check "the Windows smoke test runs windows-smoke.yml after the bundles" 'grep -qx "smoke: ./.github/workflows/windows-smoke.yml needs=bundle" "$work/wsmoke"'
check "the Windows smoke test gets the .msi artifact the bundle job uploads" 'grep -qx "smoke artifact: true" "$work/wsmoke"'
check "the Windows bundle stays best effort" 'grep -qx "windows bundle best effort: true continue-on-error: \${{ matrix.best_effort }}" "$work/wsmoke"'

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

[ "$failures" -eq 0 ] && echo "release.yml: all checks passed" || { echo "release.yml: $failures failed"; exit 1; }
