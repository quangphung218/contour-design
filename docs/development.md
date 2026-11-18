# Development

How to build, run, test and debug Contour from source. Start with
[CONTRIBUTING.md](../CONTRIBUTING.md) for the pull request rules;
[architecture.md](architecture.md) explains how the pieces fit.

## Toolchain

| Tool | Used for | Install |
|---|---|---|
| Rust 1.89+ (edition 2024) with `rustfmt`, `clippy` | everything | [rustup.rs](https://rustup.rs) |
| target `wasm32-unknown-unknown` | the UI | `rustup target add wasm32-unknown-unknown` |
| `trunk` | bundles the UI (Leptos → WASM) | `cargo install trunk --locked` or `brew install trunk` |
| Node.js 20+ | e2e tests, Tauri CLI (`npx`), commitlint | `brew install node` |
| Playwright Chromium | e2e tests | `cd crates/od-ui/e2e && npx playwright install chromium` |
| `cargo-deny`, `actionlint` | `make deny`, `make ci` | `brew install cargo-deny actionlint` |
| Chrome, Edge or Chromium | PDF, image, PPTX, MP4 export at runtime | the one on your machine |
| `ffmpeg` (optional) | MP4 export | `brew install ffmpeg` |

`make setup` checks all of these, adds the wasm target and the e2e npm
packages, and prints the install command for anything else. It never
installs system software.

On Debian or Ubuntu the desktop crate also needs WebKitGTK and friends:
`scripts/linux-deps.sh` installs the same packages CI uses.

## Make targets

`make help` prints the list. The important ones:

| Target | Does |
|---|---|
| `make setup` | check the toolchain |
| `make run` | daemon + UI at `http://127.0.0.1:7456` |
| `make dev` | daemon on `:7456` and `trunk serve` with hot reload on `:8080` |
| `make quick` | fmt, then clippy and tests of the crates changed since `origin/main` and their direct dependents ([Which check when](#which-check-when)) |
| `make check` | `cargo fmt --check`, clippy `-D warnings`, all tests |
| `make test` | tests only |
| `make lint` | fmt, clippy native and clippy for `wasm32` |
| `make e2e` | Playwright e2e (real daemon, mock agent, loopback only) |
| `make demo-gif` | record `docs/images/demo.gif` for the README (scripted demo agent, needs ffmpeg) |
| `make deny` | cargo-deny: licences, advisories, bans, sources |
| `make links` | relative links and anchors in Markdown |
| `make commitlint` | commit messages since `main` |
| `make ci` | everything CI runs, in the same order |
| `make app-debug` | debug `Contour.app` with devtools |
| `make app-release` | release `Contour.app` with the optimised UI |
| `make dmg` | release `.dmg` |
| `make install` | copy the release app to `/Applications` |
| `make version V=x.y.z` | bump the version ([release.md](release.md)) |
| `make clean` | remove the target dir, UI bundle and e2e results |

### Build settings

- **Target dir per worktree.** `make` sets
  `CARGO_TARGET_DIR=~/.cache/odrs-target-<checkout folder name>`. A shared
  target dir is unsafe across git worktrees: path crates get the same
  artifact hash in each, so cargo reuses another worktree's stale build.
  Set `CARGO_TARGET_DIR` yourself to override.
- `CARGO_INCREMENTAL=0` and `CARGO_BUILD_JOBS=4` by default (fits a 16 GB
  machine); override per command, e.g. `CARGO_BUILD_JOBS=2 make check`.
- The dev profile keeps only line tables for workspace crates and no debug
  info for dependencies, which keeps the target dir at a few GB. Set
  `CARGO_PROFILE_DEV_DEBUG=full` when you need full variable info in a
  debugger.
- The UI bundle `crates/od-ui/dist` is built once by `make ui-dist`
  (debug, ~76 MB of WASM) and reused. Release bundles always rebuild it with
  `trunk build --release` (~12 MB).

When you run `cargo` directly, export the same target dir first so it shares
the build with `make`:

```bash
export CARGO_TARGET_DIR="$HOME/.cache/odrs-target-$(basename "$PWD")"
```

## Repository layout

| Path | What |
|---|---|
| `crates/od-contracts` | serde DTOs shared by daemon, CLI and UI |
| `crates/od-store` | SQLite (WAL, migrations by `user_version`), project folders, file versions, safe paths, data folder |
| `crates/od-content` | index of skills, design templates, design systems, craft, plugins; legacy prompt assembly |
| `crates/od-runtime` | agent definitions (data), detection, spawning, stream parsers, run engine, BYOK |
| `crates/od-runs` | run registry: event ring, `events.jsonl`, `state.json`, fan-out, replay |
| `crates/od-preview` | preview serving: bridge injection, CSP, file watcher, lint, write-back of edits |
| `crates/od-export` | browser-free export: standalone HTML, ZIP, PDF/PPTX assembly, `Renderer` trait |
| `crates/od-render-cdp` | `Renderer` over the Chrome DevTools Protocol; `ffmpeg` for MP4 |
| `crates/od-media` | image, video and audio generation with user keys |
| `crates/od-live` | live artifacts: `html_template_v1`, stored under `.live-artifacts` |
| `crates/od-mcp` | stdio MCP server backed by the daemon, `od mcp install` |
| `crates/od-mcp-client` | external MCP servers for agents: config, stdio/HTTP client, OAuth 2.1 PKCE |
| `crates/od-term` | terminal tabs: PTY in the project folder, scrollback replay, process-tree kill |
| `crates/od-automation` | routines: local cron, history, no overlap |
| `crates/od-job` | Windows Job Objects to kill process trees |
| `crates/od-server` | HTTP API (axum), origin/token middleware, SSE, WebSocket |
| `crates/od-cli` | the `od` binary: daemon and client commands |
| `crates/od-ui` | Leptos CSR UI compiled to WASM by `trunk`; xterm.js in `vendor/`; e2e in `e2e/` |
| `apps/desktop` (`od-desktop`) | Tauri 2 shell running `od-server` in process |
| `content/` | bundled skills, templates, design systems, craft, plugins (licences in `NOTICE`) |
| `bridges/od-bridge.js` | script injected into previews ([protocol](../bridges/README.md)) |
| `assets/brand/` | logo sources and generator ([brand.md](brand.md)) |
| `scripts/` | helpers behind the Makefile and CI |

Crates keep the `od-` prefix from the project's first name; see
[decisions.md](decisions.md) §5.

## Running

### Daemon and browser

```bash
make run
```

The daemon listens on `http://127.0.0.1:7456` and serves the UI. Another
port, and a status check against it:

```bash
cargo run -p od-cli --bin od -- daemon start --port 7460
cargo run -p od-cli --bin od -- --daemon-url http://127.0.0.1:7460 daemon status
```

One daemon per data folder: a second daemon on the same `OD_DATA_DIR`
reports the first and exits. To try things without touching your real
projects, point it at a scratch folder:

```bash
OD_DATA_DIR="$(mktemp -d)" cargo run -p od-cli --bin od -- daemon start --port 7460
```

### UI with hot reload

```bash
make dev
```

This builds `od`, starts the daemon on `:7456`, then `trunk serve` on
`http://127.0.0.1:8080`. Trunk rebuilds the WASM on every save and proxies
`/api/` to the daemon (`crates/od-ui/Trunk.toml`). Ctrl-C stops both.

### Desktop app

```bash
make ui-dist
cargo run -p od-desktop
```

Debug builds open the webview devtools with right-click ▸ Inspect Element or
Cmd+Option+I. For a bundled app:

```bash
make app-debug        # $CARGO_TARGET_DIR/debug/bundle/macos/Contour.app
make app-release      # $CARGO_TARGET_DIR/release/bundle/macos/Contour.app
```

## Debugging

### Logs

The daemon and the desktop app log to stderr through `tracing`. The desktop
app also writes `<data folder>/logs/daemon.log` (a Finder launch has no
stderr); `od daemon start --log-file` or `OD_LOG_FILE=1` does the same for the CLI
daemon. The file rolls over at 5 MB and keeps 5 files (`daemon.log`,
`daemon.1.log` … `daemon.4.log`). A panic writes its message and stack there
first. The filter comes from `RUST_LOG` (default `info`):

```bash
RUST_LOG=debug cargo run -p od-cli --bin od
RUST_LOG=info,od_runtime=trace,od_server=debug cargo run -p od-cli --bin od
```

| What | Where |
|---|---|
| daemon log | the terminal running `od`; also `<data folder>/logs/daemon.log` with `--log-file` |
| desktop app log | `<data folder>/logs/daemon.log`, or stderr of the app started from a terminal, e.g. `RUST_LOG=debug "$CARGO_TARGET_DIR/release/bundle/macos/Contour.app/Contents/MacOS/od-desktop"` |
| agent output per run | `<data folder>/runs/<run-id>/events.jsonl`, final state in `state.json` |
| diagnostics summary | Help ▸ Report Issue, or Settings ▸ About ▸ Export diagnostics; both carry the last 200 lines of `daemon.log`, with secret-looking values (`api_key=…`, `?token=…` in a URL, `Bearer …`) redacted |
| `od mcp` server | stderr, default level `warn` |

### Data folder

Default `~/Library/Application Support/contour` (macOS),
`~/.local/share/contour` (Linux), `%APPDATA%\contour` (Windows), or
`OD_DATA_DIR`. It holds `app.sqlite`, `projects/<id>/`, `runs/<id>/`,
`app-config.json` and `0600` secret files. `daemon.lock` keeps a second
daemon out. Inspect the database with `sqlite3 app.sqlite .tables`.

### VS Code and CodeLLDB

The repository ships `.vscode/launch.json` and `.vscode/tasks.json`. Install
the [CodeLLDB](https://marketplace.visualstudio.com/items?itemName=vadimcn.vscode-lldb)
extension, then pick a configuration in Run and Debug:

| Configuration | Starts |
|---|---|
| Daemon (od) | `od daemon start --port 7456` with `RUST_LOG=info,od_server=debug,od_runtime=debug` |
| Daemon (od), scratch data folder | the daemon on `:7460` with `OD_DATA_DIR=.od-debug` (gitignored) |
| Desktop app (od-desktop) | the Tauri shell |
| Unit tests (pick a crate) | one crate's unit tests, with an optional name filter |

Each builds through cargo with the same per-worktree target dir as `make`
and runs the `ui-dist` task first. Tasks (Terminal ▸ Run Task) wrap `make
check`, `lint`, `e2e`, `dev` and `app-debug`. Breakpoints in workspace
crates work with the default line tables; set `CARGO_PROFILE_DEV_DEBUG=full`
in the launch `env` of `cargo` to see every local variable.

## Tests

### Which check when

| Check | When | What it runs |
|---|---|---|
| `make quick` | after each change while you work | fmt, then clippy and tests of the crates you changed and the workspace crates that depend on them directly; aim: under 4 minutes |
| `make check` | before you ask for review, and before every push | every crate: fmt, clippy, all tests, script tests, licences |
| `make e2e` | you changed the UI (`crates/od-ui`), the daemon's HTTP API, the preview bridge or content the UI shows | Playwright, about 13 minutes |
| `make ci` | before review on macOS, where pull request CI does not run | everything CI runs, in its order |

`make quick` compares with `origin/main` (run `git fetch` first; set
`QUICK_BASE=<ref>` for another base) and also counts uncommitted and
untracked files. A change to `Cargo.toml`, `Cargo.lock`, `.cargo/` or the
toolchain checks every crate; `content/` and `bridges/` check the crates
whose tests read them; a change to no crate runs fmt only. It uses
`cargo nextest` when installed (`cargo install cargo-nextest --locked` or
`brew install cargo-nextest`), `cargo test` otherwise.
`scripts/quick.sh --packages FILE…` prints the crates a set of files
selects.

```bash
make test                                      # everything
cargo test -p od-store                         # one crate
cargo test -p od-store first_run_takes_over    # tests whose name matches
cargo test -p od-server --test http            # one integration test file
cargo test -p od-ui --lib i18n                 # UI unit tests run natively
```

### End-to-end

`make e2e` (`crates/od-ui/e2e/run.sh`) builds the UI and `od`, then runs the
Playwright scenarios against a real daemon with a temporary data folder and
mock agents (`e2e/mock-agent`). Every request that leaves loopback is
aborted. The first run installs the npm packages; Chromium comes from
`npx playwright install chromium`.

The scenarios run `OD_E2E_JOBS` at a time (default 2, 3 on CI; `1` runs them
one after another), each with its own port, data folder and browser. Each
one's output goes to `e2e/test-results/logs/<name>.log` and is printed whole
when it ends; a failing scenario fails the run and its log is printed again
at the end. `c4-depth` checks its 18 languages `OD_E2E_C4_TABS` at a time
(default 4). `run.sh <script>…` runs only those scripts, one after another.
`OD_E2E_SKIP_BUILD=1` reuses the UI bundle, `od` and the npm packages built
by an earlier run. `OD_E2E_SHARD=K/N` runs every Nth scenario starting with
the Kth (CI splits the suite in two); `OD_E2E_RETRIES=1` runs a failed
scenario once more on its own and lists it in `test-results/logs/flaky.txt`
when it passes then. A scenario that dies, or `run.sh` stopped, leaves no
daemon behind: each scenario runs in a process group of its own, ended with
it (`make e2e-qa`, `e2e/qa/run.sh`, likewise; `QA_SKIP_BUILD=1` there).

Run one scenario after `make e2e` has built everything once:

```bash
cd crates/od-ui/e2e
export OD_UI_DIST="$PWD/../dist" OD_MOCK_AGENTS="$PWD/mock-agent" OD_RESOURCE_ROOT="$PWD/../../../content"
export OD_E2E_BIN="$CARGO_TARGET_DIR/debug/od"
node manage.e2e.mjs
```

Scenarios: `studio`, `design-system`, `library`, `manage`, `chat`, `files`,
`preview`, `c2`, `c3-runtimes`, `c4-depth`, `routes` (`<name>.e2e.mjs`). Screenshots
of failures go to `crates/od-ui/e2e/test-results/`. `screens.mjs` captures
every main screen in light and dark for design review.
`demo.mjs` (`make demo-gif`, or `run.sh demo.mjs`) records the README demo with
the scripted agent in `e2e/demo-agent`, which streams a short plan and writes
a finished page, so every recording is the same.

After changing `content/skills` or `content/design-templates`, regenerate the
golden files the content tests compare against with
`crates/od-content/tests/fixtures/regen/run.sh` (runs the original app's
loader; see `crates/od-content/tests/fixtures/README.md`).

### Browser launch

Changing how a render starts Chrome (`Session::launch`, or
`crates/od-render-cdp/src/launch_services.rs`, which starts it through
LaunchServices on macOS), or bumping `chromiumoxide`, needs the two
ignored tests that run the real Chrome inside a throwaway `.app` bundle.
Run them on macOS, outside any sandbox:

```bash
cargo test -p od-render-cdp --test launch_services -- --ignored
```

They are ignored because Chrome run from a throwaway bundle is slow to
open DevTools through LaunchServices under `make check` load (over 30 s in
2 of 6 runs; 4–7 s idle) and never opens it on the GitHub macOS runners.
What CI checks instead (`chrome-macos`, on pull requests touching
`crates/od-render-cdp` or `crates/od-cli`): the exports with the runner's
Chrome as Homebrew left it (quarantined: the child fallback), then with it
approved, where `the_installed_chrome_starts_through_launch_services`
checks that Chrome is launchd's child, not ours. When one fails, its panic lists the fake browsers and Chrome
profiles that were running.

A start through LaunchServices that fails or passes 20 s falls back to
starting Chrome as Contour's child, and so does a Chrome that is
quarantined and was never opened from Finder: LaunchServices would wait
for someone to approve its first launch (Chrome from `brew install --cask`,
as on the GitHub macOS runners). Without the fallback v0.2.0-beta.42 could
not export on those runners.

### Benchmarks

```bash
cargo bench -p od-content --bench index           # index all of content/
cargo bench -p od-runs --bench replay             # run event replay
cargo bench -p od-server --bench sse_latency      # agent stdout → SSE latency
cargo bench -p od-export --bench standalone       # standalone HTML export
```

## Continuous integration

CI has two tiers. **Pull requests run only the jobs their change needs**;
**the full tier runs every job on every OS** every night on `main` and on
each release tag, so every test runs at least once before a release.

### Pull requests and pushes to `main`

The first job, `changed paths`, compares the change with its base
(`scripts/ci-changes.sh`: the merge commit's first parent for a pull
request, the previous commit for a push) and picks the rest. Its job
summary says what it picked and why.

| Change | Jobs |
|---|---|
| code (anything below does not match) | fmt + clippy, test (ubuntu-24.04), e2e, cargo-deny; plus the two below when they apply |
| … that reaches Windows | + test (windows-2025) |
| … that reaches Chrome on macOS | + Chrome on macOS (LaunchServices) |
| version bump only | none: `changed paths` itself checks the diff is only what `make version` writes |
| docs only | none (docs.yml checks the links) |

`CI result`, the last job, passes when every job that ran passed (a
skipped job counts as passed). It is the check to require for merging, so
a version-only or docs-only pull request can merge too.

Which paths pull in which job, and why:

| Paths | Pull in | Why |
|---|---|---|
| `apps/desktop/**`, `crates/od-job/**`, `crates/od-term/**`, `crates/od-runtime/src/{detect,login_path,process,engine}.rs` | Windows tests | what the Windows job is for: the desktop shell, the Job Object, the PTY, agent lookup through `.cmd` shims and the registry Path |
| any `.rs` file with `#[cfg(…)]`, `cfg!(…)` or `#[cfg_attr(…)]` naming `windows` or `unix` (`not(windows)` too), or `target_os` / `target_family = "windows"`, before or after the change | Windows tests | code that branches on the platform; Linux builds only one branch |
| `Cargo.toml`, `Cargo.lock`, `.cargo/**`, `rust-toolchain*`, any `build.rs`, `.config/nextest.toml` | Windows tests | the build itself |
| `crates/od-render-cdp/**`, `crates/od-cli/**`, `Cargo.lock` | Chrome on macOS | how a render starts Chrome (LaunchServices exists only on macOS), the exports that use it, dependency bumps (chromiumoxide) |
| `.github/workflows/ci.yml`, `scripts/ci-changes.sh` | both | the plan itself |
| `docs/**`, Markdown outside `content/`, `crates/`, `apps/`, `bridges/` | nothing | no build reads them; Markdown under those folders is content or fixtures that tests read, so it counts as code |

A version bump is version-only when the diff holds nothing but: the
`[workspace.package]` version in `Cargo.toml`, the same old → new version
of the workspace packages in `Cargo.lock`, `CHANGELOG.md` with the new
version's section, and docs. One byte more (a dependency, an edition,
another file, `apps/desktop/tauri.conf.json`, which `make version` never
writes) and it runs as code. File names outside ASCII are read as they
are (`core.quotepath=off`). The tag build tests the release in full.

### Full tier: nightly and release

`nightly.yml` runs `ci.yml` with `only: full` at 19:00 UTC on `main`
(and from the Actions tab): fmt + clippy, test on Ubuntu, macOS and
Windows, e2e, cargo-deny, Chrome on macOS, the Windows build. A red night
shows on the Actions tab and names each job in the `CI result` table of
the run summary; nothing is opened or posted elsewhere. A manual
"Run workflow" of CI runs the same set.

`release.yml` calls `ci.yml` twice on the tagged commit: `only: release`
(every test, the Windows tests included; the draft release waits for it,
so no release goes out with a red test on any OS) and `only: windows` (the
Windows build of every target; best effort, it never blocks the release).
On pull requests the Windows tests run only for the paths above, and
nightly reports them red; the tag is where they block. See [release.md](release.md).

### Other workflows

| Workflow | Runs on | Does |
|---|---|---|
| `docs.yml` | every pull request and push to `main` | `scripts/check-links.sh`, script tests, actionlint; commitlint over the PR's commits |
| `linux-smoke.yml` | PRs touching `apps/desktop/**` or the smoke files; release (blocks the draft); manual | builds the AppImage and `.deb`, then `scripts/linux-smoke.sh` in ubuntu:22.04 and ubuntu:24.04 containers |
| `windows-smoke.yml` | PRs touching `apps/desktop/**`, the agent lookup or the smoke files; release (blocks the draft when the `.msi` built); manual | builds the `.msi`, then `scripts/windows-smoke.ps1` on windows-2025 and windows-2022 |

### Speed and stability

- The test job runs `cargo nextest run --profile ci` (test binaries side by
  side; `cargo test` runs them one after another), then the doc tests.
  `.config/nextest.toml` keeps the file-watcher tests of `od-preview`
  serial, as their mutex did under `cargo test`. Windows keeps
  `cargo test`.
- Rust caches (`Swatinem/rust-cache`) are saved from `main` only. A cache
  saved by a pull request or a tag serves only that ref; those filled the
  10 GB repository cache and pushed `main`'s out, so pull requests built
  from cold.
- e2e runs in two shards side by side (`OD_E2E_SHARD=1/2`, `2/2`: every
  other scenario of the longest-first list); in one job it was the longest
  job of a pull request (13–16 minutes).
- e2e: a scenario that fails runs once more, alone (`OD_E2E_RETRIES=1`).
  One that passes then is flaky: a warning, a "flaky" section in the job
  summary, and `flaky-row:` lines in the log for
  [qa/flaky.md](qa/flaky.md). Its first log stays as
  `test-results/logs/<name>.try1.log` in the uploaded artifact.
- Setup steps that download (apt, brew, choco, npm, Playwright browsers)
  go through `scripts/ci-retry.sh`: three tries, 15 s and 30 s apart.
  Every apt call has a time limit: `scripts/linux-deps.sh` (10 minutes a
  try; the test job installs ffmpeg through it too) and the e2e job's
  ffmpeg (5 minutes, waiting for the dpkg lock). Playwright's
  `--with-deps` has none: it runs `sudo apt-get` itself, which `timeout`
  cannot stop, and the apt-get left running held the dpkg lock. On 18/11 a mirror
  at 47 kB/s held 3 of 6 test jobs for 13 minutes in an apt call that had
  no limit. The lint and test jobs also keep the downloaded `.deb` files
  (`APT_ARCHIVES`): every run restores the newest `apt-<job>-…` cache, and
  `main` saves a new one when apt changed the folder, also after apt gave
  up, so the next run goes on from there instead of from zero. apt then
  fetches only the package lists and what changed since.
  cargo and rustup retry their own downloads. Tests are never retried, except the single e2e
  retry above.

Actions are pinned to commit SHAs, permissions are read-only, and a newer
push cancels the running workflow for the same branch. Dependabot opens at
most one grouped pull request per month for each of Cargo, GitHub Actions
and the e2e npm packages. `make ci` runs the same checks locally (without
the OS matrix).
