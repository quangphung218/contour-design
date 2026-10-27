#!/usr/bin/env bash
# Tests for .github/workflows/release.yml: runs the shell of two steps with
# fake `gh` and `npx`, and checks where the signing secrets can go.
# Needs ruby (YAML parsing; present on macOS and GitHub's Ubuntu runners).
# Usage: scripts/tests/release_workflow_test.sh   (or `make test-scripts`)
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
workflow="$here/../../.github/workflows/release.yml"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0
check() {
  if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; [ -f "$work/out" ] && sed 's/^/     /' "$work/out"; failures=$((failures + 1)); fi
}

# step <name> <file>: the `run:` script of the step with that name.
step() {
  ruby -ryaml -e '
    wf = YAML.load_file(ARGV[0])
    s = wf["jobs"].values.flat_map { |j| j["steps"] || [] }.find { |x| x["name"] == ARGV[1] }
    abort("no step #{ARGV[1]}") unless s
    print s["run"].gsub(/\$\{\{\s*matrix\.args\s*\}\}/, "--bundles app")' "$workflow" "$1" > "$2"
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
check "the Windows bundle passes a numeric MSI version" 'grep -q "scripts/msi-version.sh \"\$TAG\"" "$workflow" && grep -q "wix.*version" "$workflow"'

[ "$failures" -eq 0 ] && echo "release.yml: all checks passed" || { echo "release.yml: $failures failed"; exit 1; }
