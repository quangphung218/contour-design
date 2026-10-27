# Development

How to build, run, test and debug Contour from source. Start with
[CONTRIBUTING.md](../CONTRIBUTING.md) for the pull request rules;
[architecture.md](architecture.md) explains how the pieces fit.

## Toolchain

| Tool | Used for | Install |
|---|---|---|
| Rust 1.85+ (edition 2024) with `rustfmt`, `clippy` | everything | [rustup.rs](https://rustup.rs) |
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

```bash
make test                                      # everything
cargo test -p od-store                         # one crate
cargo test -p od-store first_run_takes_over    # tests whose name matches
cargo test -p od-server --test http            # one integration test file
cargo test -p od-ui --lib i18n                 # UI unit tests run natively
```

### End-to-end

`make e2e` (`crates/od-ui/e2e/run.sh`) builds the UI and `od`, then runs ten
Playwright scenarios against a real daemon with a temporary data folder and
mock agents (`e2e/mock-agent`). Every request that leaves loopback is
aborted. The first run installs the npm packages; Chromium comes from
`npx playwright install chromium`.

Run one scenario after `make e2e` has built everything once:

```bash
cd crates/od-ui/e2e
export OD_UI_DIST="$PWD/../dist" OD_MOCK_AGENTS="$PWD/mock-agent" OD_RESOURCE_ROOT="$PWD/../../../content"
export OD_E2E_BIN="$CARGO_TARGET_DIR/debug/od"
node manage.e2e.mjs
```

Scenarios: `studio`, `design-system`, `library`, `manage`, `chat`, `files`,
`preview`, `c2`, `c3-runtimes`, `c4-depth` (`<name>.e2e.mjs`). Screenshots
of failures go to `crates/od-ui/e2e/test-results/`. `screens.mjs` captures
every main screen in light and dark for design review.
`demo.mjs` (`make demo-gif`, or `run.sh demo.mjs`) records the README demo with
the scripted agent in `e2e/demo-agent`, which streams a short plan and writes
a finished page, so every recording is the same.

After changing `content/skills` or `content/design-templates`, regenerate the
golden files the content tests compare against with
`crates/od-content/tests/fixtures/regen/run.sh` (runs the original app's
loader; see `crates/od-content/tests/fixtures/README.md`).

### Benchmarks

```bash
cargo bench -p od-content --bench index           # index all of content/
cargo bench -p od-runs --bench replay             # run event replay
cargo bench -p od-server --bench sse_latency      # agent stdout → SSE latency
cargo bench -p od-export --bench standalone       # standalone HTML export
```

## Continuous integration

**CI on pull requests runs on Linux only.** macOS and Windows run when a
release is built, or when someone starts CI by hand (Actions tab, "Run
workflow"). On macOS the main gate is **`make ci` on your machine**: run it
before asking for review.

| Job | Workflow | Pull request, push to `main` | Release, manual run | Steps |
|---|---|---|---|---|
| fmt + clippy | `ci.yml` | Ubuntu | manual run only | `make lint` |
| test | `ci.yml` | Ubuntu | macOS (release); macOS and Ubuntu (manual) | ffmpeg (MP4 export tests), `make test` |
| build (windows) | `ci.yml` | no | Windows (never blocks a release) | `trunk build`, `cargo build --workspace --all-targets` (no tests yet) |
| e2e | `ci.yml` | Ubuntu | manual run only | `make e2e`, screenshots uploaded on failure |
| cargo-deny | `ci.yml` | Ubuntu | manual run only | licences, advisories, bans, sources (`deny.toml`) |
| docs | `docs.yml` | Ubuntu | manual run only | `scripts/check-links.sh`, script tests, actionlint |
| Linux smoke | `linux-smoke.yml` | PRs touching `apps/desktop/**` or the smoke files | release (blocks the draft); manual run | builds the AppImage and `.deb`, then `scripts/linux-smoke.sh` in ubuntu:22.04 and ubuntu:24.04 containers |
| Windows smoke | `windows-smoke.yml` | PRs touching `apps/desktop/**`, the agent lookup or the smoke files | release (blocks the draft when the `.msi` built); manual run | builds the `.msi`, then `scripts/windows-smoke.ps1` on windows-2025 and windows-2022 |
| commit messages | `docs.yml` | Ubuntu, PRs only | no | commitlint over the PR's commits |

`ci.yml` skips changes that only touch `docs/**` or Markdown; `docs.yml`
always runs. `release.yml` calls `ci.yml` twice: the macOS tests, which
every bundle waits for (each release is tested on macOS), and the Windows
build, which runs alongside and never blocks the release.

Actions are pinned to commit SHAs, permissions are read-only, and a newer
push cancels the running workflow for the same branch. Dependabot opens at
most one grouped pull request per month for each of Cargo, GitHub Actions
and the e2e npm packages. `make ci` runs the same checks locally (without
the OS matrix).
