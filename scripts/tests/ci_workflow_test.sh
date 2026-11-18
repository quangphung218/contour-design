#!/usr/bin/env bash
# Tests for .github/workflows/ci.yml and nightly.yml: runs ci.yml's "Plan"
# step (which jobs a run gets) with a stand-in scripts/ci-changes.sh, and
# checks the job wiring: `CI result` waits for every job, Rust caches are
# saved from main only, actions are pinned to a commit.
# Needs ruby (YAML parsing; present on macOS and GitHub's Ubuntu runners).
# Usage: scripts/tests/ci_workflow_test.sh   (or `make test-scripts`)
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
workflows="$here/../../.github/workflows"
ci="$workflows/ci.yml"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0
check() {
  if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; [ -f "$work/out" ] && sed 's/^/     /' "$work/out"; failures=$((failures + 1)); fi
}

ruby -ryaml -e '
  s = YAML.load_file(ARGV[0])["jobs"]["changes"]["steps"].find { |x| x["name"] == "Plan" } or abort("no Plan step")
  print s["run"]' "$ci" > "$work/plan.sh"

# A repository with two commits, and a ci-changes.sh that prints $FAKE.
repo="$work/repo"
mkdir -p "$repo/scripts"
git -C "$repo" init -q
git -C "$repo" -c user.email=t@t -c user.name=t commit -q --allow-empty -m one
git -C "$repo" -c user.email=t@t -c user.name=t commit -q --allow-empty -m two
printf '#!/bin/sh\necho "base $1" >&2\nprintf "%%s\\n" $FAKE\n' > "$repo/scripts/ci-changes.sh"
chmod +x "$repo/scripts/ci-changes.sh"

# plan EVENT ONLY [FAKE]: runs the step; its outputs, one line, in $work/out.
plan() {
  rm -f "$work/output" "$work/summary"
  (cd "$repo" && env EVENT="$1" ONLY="$2" FAKE="${3-}" BEFORE="${4-}" RUNNER_TEMP="$work" \
    GITHUB_OUTPUT="$work/output" GITHUB_STEP_SUMMARY="$work/summary" bash -e "$work/plan.sh") > "$work/log" 2>&1
  local rc=$?
  { [ -f "$work/output" ] && tr '\n' ' ' < "$work/output"; echo "rc=$rc"; } > "$work/out"
}
want() { grep -qxF -- "$1" "$work/out"; }

ALL_OS='["ubuntu-24.04","macos-15"]'
plan workflow_dispatch ""
check "a manual run runs every job" 'want "kind=code lint=true test-os=$ALL_OS e2e=true deny=true windows-test=true windows-build=true chrome=true rc=0"'
plan push full
check "only=full (nightly) runs every job" 'want "kind=code lint=true test-os=$ALL_OS e2e=true deny=true windows-test=true windows-build=true chrome=true rc=0"'
plan push release
check "only=release (tag) runs every job but Windows" 'want "kind=code lint=true test-os=$ALL_OS e2e=true deny=true windows-test=false windows-build=false chrome=true rc=0"'
plan push windows
check "only=windows runs the Windows tests and build" 'want "kind=code lint=false test-os=[] e2e=false deny=false windows-test=true windows-build=true chrome=false rc=0"'
plan push macos
check "an unknown only fails the plan" 'grep -q "rc=1" "$work/out" && grep -q "::error::unknown only" "$work/log"'

plan pull_request "" "kind=code windows=false chrome=false"
check "a code change: lint, Linux tests, e2e, deny" 'want "kind=code lint=true test-os=[\"ubuntu-24.04\"] e2e=true deny=true windows-test=false windows-build=false chrome=false rc=0"'
check "a pull request is compared with the merge commit's first parent" 'grep -qx "base $(git -C "$repo" rev-parse HEAD^1)" "$work/log"'
plan pull_request "" "kind=code windows=true chrome=true"
check "a code change reaching Windows and Chrome adds both" 'want "kind=code lint=true test-os=[\"ubuntu-24.04\"] e2e=true deny=true windows-test=true windows-build=false chrome=true rc=0"'
plan pull_request "" "kind=version windows=false chrome=false"
check "a version bump runs no other job" 'want "kind=version lint=false test-os=[] e2e=false deny=false windows-test=false windows-build=false chrome=false rc=0"'
plan pull_request "" "kind=docs windows=false chrome=false"
check "a docs change runs no other job" 'want "kind=docs lint=false test-os=[] e2e=false deny=false windows-test=false windows-build=false chrome=false rc=0"'
plan push "" "kind=code windows=false chrome=false" abc123
check "a push is compared with the commit before it" 'grep -qx "base abc123" "$work/log"'
check "the plan goes to the job summary" 'grep -q "^## Plan: changes, code" "$work/summary"'

# The wiring.
ruby -ryaml -e '
  jobs = YAML.load_file(ARGV[0])["jobs"]
  others = (jobs.keys - ["result"]).sort
  r = jobs["result"]
  puts "result needs all: #{Array(r["needs"]).sort == others}"
  puts "result if: #{r["if"]}"
  gated = (jobs.keys - ["changes", "result"]).reject { |k| Array(jobs[k]["needs"]) == ["changes"] && jobs[k]["if"].to_s.include?("needs.changes.outputs.") }
  puts "jobs not gated by changes: #{gated.join(",")}"
  caches = jobs.values.flat_map { |j| j["steps"] || [] }.select { |s| s["uses"].to_s.start_with?("Swatinem/rust-cache") }
  puts "caches: #{caches.size}, saved from main only: #{caches.all? { |s| s.dig("with", "save-if") == "${{ github.ref == \x27refs/heads/main\x27 }}" }}"
  e2e = jobs["e2e"]
  puts "e2e retries: #{e2e["env"]["OD_E2E_RETRIES"]}"
  tests = jobs["test"]["steps"].map { |s| s["run"].to_s }
  puts "nextest: #{tests.any? { |r| r.include?("cargo nextest run --workspace") }} doc tests: #{tests.any? { |r| r.include?("cargo test --workspace --doc") }}"
' "$ci" > "$work/out" 2>&1
check "CI result waits for every other job, even after a failure" 'grep -qx "result needs all: true" "$work/out" && grep -qx "result if: always()" "$work/out"'
check "every job but changes and CI result is picked by changes" 'grep -qx "jobs not gated by changes: " "$work/out"'
check "every Rust cache is saved from main only" 'grep -qE "^caches: [1-9][0-9]*, saved from main only: true$" "$work/out"'
check "e2e runs a failed scenario once more" 'grep -qx "e2e retries: 1" "$work/out"'
check "the test job runs nextest and the doc tests" 'grep -qx "nextest: true doc tests: true" "$work/out"'

# nightly.yml: the full tier on main, red when it fails, and never writes.
ruby -ryaml -e '
  wf = YAML.load_file(ARGV[0])
  on = wf["on"] || wf[true]
  ci = wf["jobs"]["ci"]
  puts "nightly: #{ci["uses"]} only=#{ci["with"]["only"]} cron=#{on["schedule"].map { |c| c["cron"] }.join(",")}"
  puts "permissions: #{wf["permissions"]}"
  puts "report needs ci always: #{Array(wf["jobs"]["report"]["needs"]) == ["ci"] && wf["jobs"]["report"]["if"] == "always()"}"
' "$workflows/nightly.yml" > "$work/out" 2>&1
check "nightly runs ci.yml with only=full on a schedule" 'grep -qE "^nightly: ./.github/workflows/ci.yml only=full cron=.+" "$work/out"'
check "nightly reads only (no issue, no comment)" 'grep -qx "permissions: {\"contents\"=>\"read\"}" "$work/out" || grep -qx "permissions: {\"contents\" => \"read\"}" "$work/out"'
check "nightly reports even when CI failed" 'grep -qx "report needs ci always: true" "$work/out"'
check "nightly opens no issue" '! grep -qiE "gh issue|issues:" "$workflows/nightly.yml"'

# Every action is pinned to a full commit SHA.
grep -hE '^\s*-?\s*uses:' "$workflows"/*.yml | grep -vE 'uses: \./' | grep -vE '@[0-9a-f]{40}( |$)' > "$work/out"
check "every action is pinned to a commit SHA" '[ ! -s "$work/out" ]'

[ "$failures" -eq 0 ] || { echo "ci workflow: $failures check(s) failed" >&2; exit 1; }
echo "ci workflow: all checks passed"
