# Architecture

A one-page summary. Behaviour details are in [`spec.md`](spec.md) (§5 is the original architecture proposal); settled decisions are in [`decisions.md`](decisions.md).

## One API, three clients

```
 browser     ─┐                        ┌─ od-store   (SQLite + project folders)
 desktop app ─┼─ HTTP / SSE / WS ─ od-server ─┼─ od-runs    (run events)
 CLI, MCP ────┘   127.0.0.1                 ├─ od-runtime (spawn agent CLI / BYOK)
                                            ├─ od-preview, od-export, od-render-cdp
                                            └─ od-term, od-automation, od-media, od-mcp-client
```

- **Daemon** (`od-server`, run by `od` or embedded in the desktop app) holds all state; clients keep nothing of their own. It binds loopback only by default; binding externally requires `OD_API_TOKEN`. Middleware rejects unknown origins.
- **UI** (`od-ui`) is Leptos CSR compiled to WASM and served by the daemon itself as static files. The UI only calls `/api/*`; there is no other shortcut.
- **CLI** `od` and the **MCP server** (`od mcp`) are thin clients of the same API (dual-track rule: every feature in the UI is also callable from the CLI/agent).
- **Desktop app** (`apps/desktop`, Tauri 2) runs `od-server` in-process on a random loopback port and opens a webview pointing at it. Tauri only handles the native parts: folder picker dialog, menus. The IPC capability is granted only to the daemon's exact origin.

## Lifecycle of a run

1. The UI or CLI sends `POST /api/runs` (project, agent, message).
2. `od-content` assembles the legacy prompt: prompt-injection guard → mode → discovery → designer charter → design system → craft → skill/template → plugin → overrides (spec §2.4).
3. `od-runtime` finds the agent on `PATH`, builds arguments from the agent's data definition (permission flags, resume, model), and spawns it in a process group with the project folder as cwd and an `OD_*`-filtered env.
4. A parser for the agent's stream format (Claude stream-json, Codex exec-json, ACP, text) turns stdout into `AgentEvent`s.
5. `od-runs` writes events to a ring buffer and `events.jsonl`, and streams them over SSE `GET /api/runs/:id/events` (resumable with `Last-Event-ID`).
6. The agent writes files straight into the project folder. `od-preview` watches the folder and tells the UI to reload the preview; HTML files have version history by source (AI, manual edit, restore).
7. End of run: `state.json` records the final state. The UI calls `GET /api/runs/:id/deliverable` to check whether the run really produced an output file; artifact lint runs when the user, the agent or `od lint` asks for it.

## Preview

The preview is a sandboxed iframe. `od-preview` injects `bridges/od-bridge.js` into the HTML before any author script; the bridge talks to the UI via `postMessage` (`od:*`): element picking for comments, inspecting and editing style/text (written back to source), palette, tweaks (declared by the artifact in a `script[type="application/json"][data-od-tweaks]` block), deck. The full protocol is in [`bridges/README.md`](../bridges/README.md).

## Data

`OD_DATA_DIR` holds `app.sqlite` (projects, conversations, messages, file versions), `projects/<id>/` (project files, except projects opened from an existing folder), `runs/<id>/` (`events.jsonl`, `state.json`), `app-config.json` and `0600` secret files (BYOK, MCP). A `daemon.lock` lock file ensures one daemon per data directory.

## Child processes

Agents, terminal tabs and stdio MCP servers each run in their own process group (Windows: a Job Object via `od-job`). Cancelling a run, closing a tab or shutting down the daemon/app stops the whole process tree: SIGTERM, wait, then SIGKILL.

## Export

`od-export` does everything that needs no browser (self-contained HTML, ZIP, assembling PDF/PPTX from images). Anything that needs rendering goes through the `Renderer` trait, implemented by `od-render-cdp` using the Chrome/Edge/Chromium already installed on the machine; MP4 also needs `ffmpeg`.

## Content

`content/` (skills, design templates, design systems, craft, plugins, prompt templates) is indexed at startup (`od-content`), together with user folders under `OD_DATA_DIR`. The desktop app bundles `content/`. Provenance and licenses are in [`NOTICE`](../NOTICE).
