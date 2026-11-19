# Status

What works in Contour today, how each part was checked, and what has not been checked. Last updated
2026-11-19, for `0.2.0-beta.50`. `main` at `2064d77` (#265) adds what is under Unreleased in the
changelog (the quit dialog for running agents, suggestions under a turn), checked by tests only so far. Each user-visible change is in
[CHANGELOG.md](../CHANGELOG.md); the feature-by-feature comparison with OpenDesign is in
[parity.md](parity.md).

When a change makes a row here wrong, update it in the same pull request.

## How a change is checked

| Check | Where | What it runs |
|---|---|---|
| Local gate | `make check` (`make ci` on macOS before a release) | fmt, clippy `-D warnings`, the workspace tests and the script tests |
| Pull request, code | [`ci.yml`](../.github/workflows/ci.yml), jobs picked by `scripts/ci-changes.sh` | lint, tests on Linux, e2e (Playwright, 2 shards), `cargo-deny`; the Windows tests and the Chrome tests on macOS when the change reaches them |
| Pull request, docs | [`docs.yml`](../.github/workflows/docs.yml) | relative links (`scripts/check-links.sh`), script tests, actionlint, commit messages |
| Nightly and manual | `ci.yml` with `only: full` | every job on Linux, macOS and Windows |
| Release tag | [`release.yml`](../.github/workflows/release.yml) | every CI test on the tagged commit, the bundles, the macOS sealed-signature check, the Linux and Windows smoke tests; the draft waits for them |
| By hand | [docs/qa/](qa/) | a person (or an agent driving the real app) runs a released build; each run is a dated note |

The e2e tests start a real daemon with a mock agent and fail on any request that leaves the machine. 22
specs live in `crates/od-ui/e2e/`.

## What works, and how it was checked

| Area | State | Checked by |
|---|---|---|
| Install on macOS | The universal `.dmg` installs by drag and drop. Gatekeeper says "“Contour” Not Opened" once; the README's `xattr` step clears it. | By hand on the released beta.49 and beta.50 DMG, macOS 26, Apple silicon ([beta.49](qa/first-run-beta49-native.md), [beta.50](qa/first-run-beta50-native.md)). |
| macOS signature | The app is signed ad-hoc with its resources sealed, so macOS keeps "Allow" for Documents across runs; it asks again once after each update. | `codesign --verify --deep --strict` and `codesign -dv` on the beta.50 app (`Signature=adhoc`, `Sealed Resources version=2 files=5594`); the release's "Sealed signature" step runs `scripts/macos-signature.sh` on the `.dmg` and the updater archive. |
| Connect an agent | Contour finds Claude Code, tests it and says when macOS may be waiting on a permission dialog. | By hand on beta.50: one Documents dialog, Allow, then the test passed in 5.4 s and again after a relaunch with no dialog. |
| First design from a brief | A one-sentence brief on Home becomes a page in the project folder, shown live. | By hand with the real Claude Code CLI: beta.47 (about 2 min 20 s from opening the app to three exports, 98 s of it the agent) ([note](qa/first-run-beta47.md)); beta.50 (page written in 85 s). |
| Export HTML, PDF, PNG | Through the native Save panel; PDF and PNG wait for entrance animations. | By hand on beta.49 and beta.50 (each file saved about 1 s after Save); `od-export` tests; e2e `preview.e2e.mjs`; the Chrome tests on macOS (`chrome-macos` job) when export code changes. |
| Export PPTX, MP4, ZIP, vector PDF | Available from the Export menu and `od export --format`. | Automated tests only (`crates/od-export`, e2e). Not run by hand on a released build. |
| Comment, edit, versions | Comments pinned to elements, manual edits written back to the source, per-file history with restore. | e2e `preview.e2e.mjs`, `chat.e2e.mjs`, `studio.e2e.mjs`; `crates/od-store/src/history_tests.rs`; the wave C live run in [parity.md](parity.md#wave-c-live-run-results). |
| Other agents (25 CLIs, API key) | Detected on `PATH`; each speaks its own stream format. API key mode has 28 presets. | `crates/od-runtime/tests/` (`engine*.rs`, `golden*.rs` against recorded fixtures); e2e `c3-runtimes.e2e.mjs` runs a turn through a mock `qodercli`. Only Claude Code was run live by hand. |
| Linux (Ubuntu 22.04, 24.04 x64) | `.deb` and AppImage install and open under X11. | Each release: `linux-smoke.yml` on clean Ubuntu 22.04 and 24.04 containers under Xvfb: install, `/api/health`, an agent turn, a PDF export, the MCP server through Integrations. |
| Windows (10, 11 x64) | `.msi` installs, opens, runs an agent turn and a PDF export, uninstalls; upgrades over the previous release. | Each release: `windows-smoke.yml` on Windows Server 2025 and 2022 runners, plus `scripts/windows-upgrade-smoke.ps1`. Every pull request that reaches Windows code runs the workspace tests on Windows. |
| Check for Updates | Asks GitHub Releases only when clicked. | e2e `updates.e2e.mjs` against a loopback stub; by hand on beta.20 and beta.21 ("You're up to date"). |
| No telemetry | No analytics, crash-reporting or tracing client in the code or its dependencies. | Every e2e spec fails on a request that leaves loopback; `cargo-deny` on each code pull request. |
| Parity with OpenDesign | 206 of 206 scored features; 21 rows left out on purpose. | Measured 2026-10-24 on `f4c455b`, 10 Playwright scenarios run live ([parity.md](parity.md)). |

## Partly there

- **Windows and Linux are checked by machines, not people.** The smoke tests run on Windows Server and in
  Ubuntu containers under Xvfb, not on a Windows 10 or 11 desktop or a Linux desktop session.
- **Agents other than Claude Code** are tested against recorded output and mock CLIs. A new version of
  an agent CLI can change its output before the tests notice.
- **No Developer ID or Windows code signing.** macOS and Windows warn once on first open.
- **Terminal tabs and the tests that drive a mock agent CLI (a `sh` script)** run on macOS and Linux
  only; ConPTY on Windows is untested.

## Not checked yet

- **Update now through Restart** from the previous release, on macOS (both architectures), the AppImage
  and the `.msi`. CI cannot do it; the last attempt by hand (QA run main36) was not run because the
  screen was locked.
- **Export PPTX and MP4** on a released build by hand.
- **Other Linux distributions** with WebKitGTK 4.1, and Wayland sessions.

## Known limitations

- **Linux means Ubuntu 22.04 LTS and 24.04 LTS on x64.** For a blank window under Wayland see
  [troubleshooting](troubleshooting.md#the-window-is-blank-or-white-wayland-nvidia). There is no ARM
  build.
- **Windows means Windows 10 and 11 on x64.** The installer is unsigned and there is no ARM64 build.
  Onboarding links each agent's setup page rather than giving an install command.
- **External MCP servers reach Claude, ACP agents and OpenCode only**, as in OpenDesign; ACP takes
  stdio servers only.
- **Fully detached processes can survive.** An MCP stdio helper that double-forks, or a terminal job
  that calls `setsid` and loses its parent within 2 seconds, keeps running after its tab or the daemon
  stops; macOS has no subreaper to stop it.
- **Direct text editing applies to text-only elements.** Elements with child tags or generated by
  scripts answer 409 with the reason.
- **No background auto-update.** Check for Updates ▸ Update now installs a newer release when you click
  it, except the `.deb` and source builds: install the new `.deb`, or `git pull` and rebuild.
- **Export to PDF, images, PPTX and MP4 needs Chrome, Edge or Chromium** already installed; Contour
  never downloads a browser.
