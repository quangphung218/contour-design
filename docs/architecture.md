# Architecture

The codebase map. Read this first, then the `CLAUDE.md` of the subsystem you change.
Behaviour is in [`spec.md`](spec.md); settled decisions are in [`decisions.md`](decisions.md).
Anchors are path:function (or Type::method); tests are path::test_name.

## Processes

```
 desktop shell (Tauri) ─┐  in-process
 browser ───────────────┼── HTTP / SSE ── od-server (daemon) ──┬─ agent CLI child (one per run)
 od CLI, od mcp ────────┘   127.0.0.1                          ├─ Chrome / Edge (export only, CDP)
                                                               └─ PTYs, stdio MCP servers, ffmpeg
```

| Process | Started by | Talks over |
|---|---|---|
| Desktop shell `od-desktop` | the user | runs od-server in-process on `127.0.0.1:0`; webview opens that origin |
| Daemon `od daemon start` | `crates/od-cli/src/lib.rs:run_daemon` → `crates/od-server/src/lib.rs:serve` | `127.0.0.1:7456` (`DEFAULT_PORT`) unless `OD_HOST`/`OD_PORT` |
| od-ui (Leptos CSR, WASM) | served as static files by `crates/od-server/src/routes/ui.rs:routes` | same-origin `/api/*`, SSE, iframe `postMessage` (`od:*`) |
| Agent CLI child | `crates/od-runtime/src/process.rs:spawn`, own process group | stdout stream; calls back via `od` / `od mcp` with `OD_DAEMON_URL`, `OD_TOOL_TOKEN` |
| `od mcp` stdio server | the agent | daemon HTTP; finds it via `crates/od-mcp/src/locate.rs:daemon_urls` |
| Chrome renderer | `crates/od-render-cdp/src/lib.rs:Session::launch` | CDP, headless, throwaway profile |

Guards between them:
- One daemon per data dir: `daemon.lock` (`crates/od-server/src/lock.rs:DaemonLock::acquire`, `DaemonLock::record_url`).
- The desktop app also writes `daemon-app.url` (`apps/desktop/src/data_lock.rs:record_app_url`) so a new launch can wait for its own quitting copy.
- Origin and token checks: `crates/od-server/src/guard.rs:origin_guard`, `token_guard` (`OD_API_TOKEN` is required for a non-loopback bind).
- Desktop IPC is granted only to the daemon origin: `apps/desktop/src/main.rs:ui_capability`, `allowed_navigation`.
- Child processes (agents, PTYs, MCP servers) are killed as a tree: process group on Unix, Job Object on Windows (`od-job`).

## Crates

| Crate | Owns | Internal deps |
|---|---|---|
| `od-contracts` | serde DTOs shared by daemon, CLI, UI | none |
| `od-store` | `app.sqlite`, project folders, file history, safe paths, drafts ledger, data dir | contracts, fs |
| `od-runs` | run registry: event ring, `events.jsonl`, `state.json`, SSE replay | contracts |
| `od-job` | process-tree kill (POSIX groups, Windows Job Objects) | none |
| `od-runtime` | agent definitions, detection, spawn, stream parsers, run engine, BYOK | contracts, job, runs, store |
| `od-content` | index of skills, templates, design systems, craft, plugins; prompt assembly | contracts, fs |
| `od-fs` | file access in folders an agent can write: no link followed, no FIFO waited on (`nofollow`); one lock per user root, tagged read and compare-and-write, crash-safe remove and put of a tree (install, replace, restore) and start-up `recover` (`user_store`) | none |
| `od-preview` | bridge injection, CSP, preview scopes, file watcher events, lint | contracts, render-cdp, store |
| `od-export` | browser-free export (HTML, ZIP, PDF/PPTX from images), `Renderer` trait | none |
| `od-render-cdp` | `Renderer` over CDP with the installed Chrome; ffmpeg | export |
| `od-term` | PTY terminal tabs | job |
| `od-automation` | local cron routines | none |
| `od-live` | live artifacts | contracts, store |
| `od-media` | media generation with user keys, `media-config.json` | none |
| `od-mcp-client` | external MCP servers for agents, OAuth PKCE | job |
| `od-mcp` | `od mcp` stdio server, `od mcp install` | store |
| `od-server` | axum HTTP API (`router`, `serve`) | all of the above except od-cli and od-job (transitive) |
| `od-cli` | the `od` binary: daemon start and API client | server, runtime, store, … |
| `od-ui` | Leptos CSR WASM app (built with trunk) | contracts only; talks HTTP |
| `apps/desktop` | Tauri shell, in-process daemon, menus, dialogs, updater | cli, server, runtime, store, … |

Subsystem notes: [od-ui](../crates/od-ui/CLAUDE.md), [od-server](../crates/od-server/CLAUDE.md),
[od-runtime](../crates/od-runtime/CLAUDE.md), [od-store](../crates/od-store/CLAUDE.md),
[od-render-cdp](../crates/od-render-cdp/CLAUDE.md), [apps/desktop](../apps/desktop/CLAUDE.md).

## Main flow

### 1. Install and first run

- Data dir: `crates/od-store/src/data_dir.rs:DataDir::from_env` (`OD_DATA_DIR`, else the platform data dir).
- Desktop: `apps/desktop/src/main.rs:main` opens a splash window → `start_daemon`
  → `data_lock::acquire_waiting` → `Store::open` (migrations in `crates/od-store/src/db.rs:migrate`)
  → `ContentStore::load` → bind `127.0.0.1:0` → `data_lock::record` → `od_server::router`
  → `ui_capability(origin)` → the window navigates to the daemon URL.
- CLI: `crates/od-cli/src/lib.rs:run_daemon` → `crates/od-server/src/lib.rs:serve`
  (`DaemonLock::acquire` → `Store::open` → `ContentStore::load` → bind → `record_url` → `open_runs`).
- UI first run: `crates/od-ui/src/app/onboarding.rs:Onboarding`.

### 2. Connect an agent

- UI: `crates/od-ui/src/app/mod.rs:Shell::reload_agents` (`GET /api/agents`), `Shell::rescan_agents`;
  picker `crates/od-ui/src/app/onboarding.rs:AgentPicker`; choice saved by `Shell::save_config`.
- Server: `crates/od-server/src/routes/runs.rs:agents` → `crates/od-runtime/src/detect.rs:detect_all_in`
  → `resolve_bin` (PATH, plus the login-shell PATH from `EngineConfig::with_login_path`).
- Connection test: `crates/od-server/src/routes/manage/settings.rs:test_connection`.

### 3. Create a design

- UI: `crates/od-ui/src/app/new_project.rs:NewProjectPanel` → `POST /api/projects`.
- Server: `crates/od-server/src/routes/projects.rs:create` → `crates/od-store/src/store.rs:Store::create_project`
  → a `projects` row and `<data>/projects/<id>/` (an imported folder keeps its own path in `baseDir`).

### 4. Stream a turn

1. UI: `crates/od-ui/src/app/studio.rs:StudioCtx::send` → `send_at` → `POST /api/runs`.
2. `crates/od-server/src/routes/runs.rs:create` → `prepare_and_launch`.
3. Prompt: `crates/od-content/src/prompt.rs:compose_instructions`, `compose_user_message`;
   skill staging `crates/od-runtime/src/stage.rs:stage_skill`; env `crates/od-runtime/src/env.rs:agent_env`.
4. Spawn: `crates/od-runtime/src/engine.rs:RunEngine::launch` → `drive` → `attempt`
   → `crates/od-runtime/src/process.rs:spawn` (API-key runs: `attempt_byok`).
5. Parse: `crates/od-runtime/src/parse/mod.rs:parser_for` → `StreamParser::line`
   → `crates/od-runtime/src/engine.rs:apply_events`.
6. Events: `crates/od-runs/src/run.rs:Run::emit` (ring + `events.jsonl`; `write_state` on lifecycle).
7. Chat rows: `crates/od-server/src/routes/runs.rs:persist_run` → `crates/od-store/src/chat.rs:append_event_batch`.
8. SSE: `crates/od-server/src/routes/runs.rs:events` → `Run::subscribe` (resumes from `Last-Event-ID`).
9. UI: `crates/od-ui/src/app/studio.rs:StudioCtx::attach` → `crates/od-ui/src/app/sse.rs:Sse::open`
   → `crates/od-ui/src/events.rs:decode_frame` → `crates/od-ui/src/chat.rs:AssistantTurn::apply`.
10. End of run: `crates/od-ui/src/app/preview_tools.rs:check_deliverable`
    → `crates/od-server/src/routes/preview_tools.rs:deliverable`.

### 5. Preview

- The agent writes files straight into the project folder.
- Watch: `crates/od-server/src/routes/preview.rs:events` → `crates/od-preview/src/events.rs:ProjectEvents::subscribe`.
- UI: `crates/od-ui/src/app/studio.rs:StudioCtx::watch_project` → `StudioCtx::reload_preview`.
- The iframe (`crates/od-ui/src/app/studio.rs:PreviewPane`) loads `/api/projects/{id}/raw/<path>?bridge=1`.
- Serve: `crates/od-server/src/routes/raw.rs:raw`, wrapped by `crates/od-server/src/routes/preview.rs:raw_bridge`
  → `crates/od-preview/src/inject.rs:inject_bridge_keyed` (injects `bridges/od-bridge.js` before author scripts).
- UI side of the bridge: `crates/od-ui/src/app/bridge.rs:post`, `decode`. Protocol: [`bridges/README.md`](../bridges/README.md).

### 6. Export

- UI: `crates/od-ui/src/app/export_menu.rs:run_export` → `POST /api/projects/{id}/export/{kind}`.
- Server: `crates/od-server/src/routes/export/mod.rs:routes` builds `CdpRenderer::new`;
  handlers `export_html`, `archive`, `export_pdf`, `export_image`, `export_pptx`;
  MP4 in `crates/od-server/src/routes/media/mp4.rs:export_mp4`.
- No browser: `od-export` (standalone HTML, ZIP, PDF/PPTX from PNGs).
- Browser: `crates/od-export/src/renderer.rs:Renderer` → `crates/od-render-cdp/src/lib.rs:CdpRenderer`
  → `Session::open` → `Session::launch` (`find_chrome`).

## State: where it lives, who writes it

| State | Where | Writer |
|---|---|---|
| App config | `<data>/app-config.json` | `crates/od-server/src/routes/ui.rs:put_config` (merge patch under `CONFIG_LOCK`), `patch_config` |
| Secret configs (0600) | `agent-cli-env.json`, `mcp-config.json`, `mcp-tokens.json`, `media-config.json` | `crates/od-server/src/routes/manage/settings.rs`, `od-mcp-client`, `od-media` |
| Database | `<data>/app.sqlite` | `od-store` only (`crates/od-store/src/db.rs:migrate`) |
| Project files | `<data>/projects/<id>/` or the imported folder | the agent (direct), `Store::write_file`, `Store::upload_files`, manual edit |
| File history | `<project>/.file-versions/<hash>/manifest.json` + snapshots | `crates/od-store/src/versions.rs:create_version`, via `Store` |
| Drafts ledger | tables `draft_uploads`, `draft_sessions_released` | `crates/od-store/src/drafts.rs:record_draft_upload`, `release_draft_session`, `sweep_draft_uploads` |
| Unsent drafts in the UI | memory (`StudioCtx.comment_drafts`); chat composer text in `localStorage` | `crates/od-ui/src/app/preview_comments.rs`, `crates/od-ui/src/app/composer.rs:draft_key` |
| Run files | `<data>/runs/<id>/events.jsonl`, `state.json` | `od-runs` (`Run::emit`, `write_state`); read back by `Runs::load` |
| Daemon lock | `<data>/daemon.lock` | `DaemonLock::acquire`, `record_url` (CLI daemon and app) |
| App URL | `<data>/daemon-app.url` | desktop only, `apps/desktop/src/data_lock.rs:record_app_url` |
| Chrome profiles | temp dir, one per render | `crates/od-render-cdp/src/lib.rs:Profile::new`; cleaned by `sweep_stale_profiles` |

`localStorage` in od-ui holds UI state only (layout, tabs, unsent text). The daemon never reads it.

## Diagnostics

Nothing leaves the machine: the user copies or attaches what these give.

- Log: `crates/od-server/src/logs.rs:init` → stderr and, in the app or with
  `--log-file`/`OD_LOG_FILE`, `<data>/logs/daemon.log` (5 files of 5 MB). The
  file layer has its own field formatter (`PlainFields`), else span fields
  arrive with the console's ANSI colors.
- Request ids: `crates/od-server/src/request_id.rs:tag` (outermost layer of
  `router`) runs each request in a span `req{request_id, method, path}` and
  answers with `x-od-request-id`; it remembers the last request that failed, and
  `crates/od-server/src/routes/runs.rs:persist_run` the last run that failed (`remember_failed`).
  Work moved to its own task keeps the span: od-server spawns through
  `crates/od-server/src/spawn.rs` (`spawn_in_span`, `spawn_blocking_in_span`;
  `tests/spawn_in_span.rs` refuses a direct `tokio::spawn`/`spawn_blocking`
  outside its start-up list), and `runs.rs` instruments its `TaskTracker` tasks;
  `crates/od-runtime/src/engine.rs:RunEngine::launch` adds `run{run_id, agent, project}`,
  and `finish` logs a failed run's category, exit code and reason (one line, 300 characters) there;
  `crates/od-render-cdp/src/lib.rs` instruments the CDP handler and request
  interception tasks. Error answers log their code and message in the span
  (`crates/od-server/src/error.rs:log_answer`).
  - `crates/od-server/tests/request_id.rs::every_line_of_a_failed_export_carries_its_request_id`
  - `crates/od-server/tests/request_id.rs::a_runs_lines_carry_its_run_id_and_the_request_that_started_it`
  - `crates/od-server/tests/request_id.rs::a_spawned_tasks_lines_carry_the_request_id`
  - `crates/od-server/tests/spawn_in_span.rs::every_task_spawned_in_src_keeps_the_callers_span`
- UI: `crates/od-ui/src/app/api.rs:error_of` keeps the header in `HttpError.request_id`;
  `failure_with_ref` adds "(Ref: …)" to export failures; a failed run's status line
  shows its run id's first 8 characters (`crates/od-ui/src/app/chat_view.rs:run_ref`).
- Report Issue `GET /api/diagnostics/summary` (`crates/od-server/src/routes/c2/help.rs:summary`)
  and Export diagnostics `GET /api/diagnostics/export`
  (`crates/od-server/src/routes/manage/diagnostics.rs:export`) take `?id=`
  (else the last failed request or run) and put its lines first (`log_report`); all lines pass `scrub_line`.
- `od doctor` (`crates/od-cli/src/doctor_cmd.rs`) and Settings ▸ About ▸ Check setup
  (`POST /api/doctor`, behind the origin guard: it starts Chrome and the agents; one run at a time) share `crates/od-server/src/doctor.rs`; each check takes its
  inputs as arguments and has a pass and a fail test there. Exit 1 on any `fail`.
  `od doctor` asks the daemon that answers to run the checks (`POST /api/doctor`, its own data folder and agents)
  and runs them itself only when none does.

## Content

`content/` (skills, templates, design systems, craft, plugins, prompt templates) is indexed at start
by `od-content`, together with user folders under `OD_DATA_DIR`. The desktop app bundles it.
Provenance and licenses are in [`NOTICE`](../NOTICE).
