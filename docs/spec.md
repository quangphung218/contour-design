# Behaviour spec: rewriting the original app as a local-only Rust app

- Source analysed: repo `nexu-io/open-design` (the **original app**), commit `e03d5c2`, Apache-2.0 license. Read from source directly, not reverse-engineered.
- Target repo: `contour-design` (this repository; no code yet when the spec was written).
- Scope: local app only, single user. **Drop all** login, accounts, cloud, billing, team, share, collab, publish/deploy, telemetry.
- Source tag on each claim:
  - **[S]**: read from source, with path (relative to the original app's repo root).
  - **[D]**: docs in the repo (`docs/`, `AGENTS.md`, README).
  - **[Inf]**: inferred from analysis, not verified by running.
- The original app was not run. All behaviour descriptions come from reading code. The spec only describes; it does not paste code or UI strings verbatim.
- Size of the original app, for scale: daemon about 306k lines of TS, web about 455k lines of TS/TSX [S]. `server.ts` alone is about 18.5k lines, `FileViewer.tsx` about 20k lines [S]. The rewriter **should follow the behaviour in this spec**, not the file structure.

---

## 1. Feature map

Priority levels:
- **P0**: working MVP: brief → agent generates an HTML prototype → live preview → keep editing → export.
- **P1**: do right after the MVP, high value.
- **P2**: later.
- **DROP**: login, cloud, billing, team, share, telemetry, AMR/Vela, paid cloud plans.

### 1.1 Project and chat core

| Feature | What the user sees | Original app source | Level |
|---|---|---|---|
| Home: brief box | Large input; Enter creates a project, opens it, and sends the brief as the first turn | `apps/web/src/components/HomeView.tsx`, `HomeHero.tsx`, `components/composer/*` [S] | P0 |
| Chip rail (output type) | Row of chips under the input: prototype, deck, document, image, video, HyperFrames, audio, live-artifact… A chip picks the surface, default skill, and project kind | `components/home-hero/chips.ts`, `runtime/home-intent.ts` [S]; glossary `CONTEXT.md` [D] | P0 for prototype and deck, P2 for the rest |
| Pick design system at creation | Picker in the composer. Changing the system makes the next render use the new tokens | `components/DesignSystemPicker.tsx` [S] | P0 |
| Pick starting template | Template picker; the chosen template **replaces** the default skill as the main `skillId` | `components/home-hero/TemplatePicker.tsx`; `docs/modes.md` [D] | P0 |
| Project list | Grid of projects with cover image and run-status dot. Create, rename, duplicate, delete (with confirmation) | `components/DesignsTab.tsx`, `components/project-actions/*` [S] | P0 (cover image P1) |
| Open existing folder as project | Pick a directory via native dialog; the project points straight at that directory, no copy | `POST /api/import/folder`, `apps/daemon/src/import-export-routes.ts` [S] | P1 |
| Project screen (Studio) | Split view: chat on the left, file workspace and preview on the right | `components/ProjectView.tsx` [S] | P0 |
| Chat stream | Assistant turn streams in: text, thinking blocks, tool lines, summary of written files, Markdown with code highlighting | `components/ChatPane.tsx`, `AssistantMessage.tsx`, `ToolCard.tsx` [S] | P0 |
| Stop | Stop button cancels the running run | `POST /api/runs/:id/cancel` [S] | P0 |
| Keep editing (next turn) | Type again in the same conversation; the agent resumes its session when possible | `apps/daemon/src/agent-session-resume.ts` [S] | P0 |
| Execution record and TodoWrite | Checklist of steps in the turn. A "step N/M" pill floats above the composer while the run is live; N is the current step | `components/chat/ExecutionShell.tsx`, `runtime/chat/plan-pill.ts` [S]; `AGENTS.md` section Chat UI [D] | P1 |
| Question form | The agent inserts a `<question-form>` block (JSON of choice or text questions). The UI renders it as a form; submitting turns the answers into the next user message | `apps/web/src/artifacts/question-form.ts`, `components/QuestionForm.tsx` [S]; `AGENTS.md` section Asking the user questions [D] | P0 |
| Queue and steer | Sending while a run is live queues the message; edit, delete, drag to reorder. The steer button pushes a message into the middle of the turn (Claude only) | `components/chat/QueuedSendStack.tsx`, `apps/daemon/src/runtimes/run-steering.ts` [S] | P1 |
| Multiple conversations, fork | Conversation list in the project; fork from a message | `routes/project/conversations.ts` [S] | P1 |
| Session mode | Pick design, chat or plan; changes the head of the system prompt | `components/SessionModeToggle.tsx` [S] | P2 |
| Side chat tab | Secondary chat tab with the same agent | `components/workspace/SideChatTab.tsx` [S] | P2 |

### 1.2 Workspace, preview, editing

| Feature | What the user sees | Original app source | Level |
|---|---|---|---|
| File tree | File panel with breadcrumb, upload, copy path, grouping by artifact | `components/DesignFilesPanel.tsx`, `components/design-files/*` [S] | P0 |
| Quick switcher | Cmd/Ctrl+P, fuzzy search, recently opened files first | `components/QuickSwitcher.tsx` [S] | P1 |
| Workspace tabs | File, terminal, browser, sketch tabs. Cmd/Ctrl+T, Cmd/Ctrl+W, Ctrl+Tab, Cmd/Ctrl+1–9 | `components/FileWorkspace.tsx`, `WorkspaceTabsBar.tsx` [S] | P1 (file tabs only), P2 (other tab kinds) |
| HTML preview | Sandboxed iframe. Two load modes: URL or srcDoc (section 4.1). Desktop, tablet, phone device frames | `components/file-viewer-render-mode.ts`, `runtime/srcdoc.ts` [S] | P0 |
| Live reload | When a file changes on disk the preview reloads itself | `apps/daemon/src/project-watchers.ts`, `providers/project-events.ts` [S] | P0 |
| Other viewers | Image, SVG, video, audio, Markdown, text/code, PDF/DOCX/PPTX/XLSX (text extraction), binary files | `FileViewer.tsx` (sub-viewers), `apps/daemon/src/document-preview.ts` [S] | P0 for image, SVG, text, Markdown; P1 for the rest |
| Deck viewer | Thumbnail rail, prev/next/first/last via keys, present mode (Esc exits), speaker notes | `components/DeckThumbnailRail.tsx`, `runtime/slide-nav.ts`, `runtime/speaker-notes.ts` [S] | P0 for navigation, P1 for thumbnails, present and notes |
| Comments on preview | Enable comment mode, hover or click an element, pin a note. Notes are sent with the next turn as context | `runtime/comment-bridge.ts`, `routes/project/comments.ts` [S] | P1 |
| Inspect, edit style | Select an element, edit its style live (set, reset, extract) | `runtime/srcdoc.ts` (`od:inspect-*`) [S] | P2 |
| Direct text editing | Edit text right in the preview, written back to source | `edit-mode/bridge.ts`, `edit-mode/source-patches.ts` [S] | P2 |
| Tweaks panel | The artifact declares its own parameter panel; the host shows it | `design-templates/tweaks/`, `srcdoc.ts` (`od:tweaks-*`) [S] | P2 |
| Palette swap | Quickly swap the preview palette among 5 presets | `srcdoc.ts` (palette bridge) [S] | P2 |
| Annotation drawing | Draw over the preview and send a screenshot to the agent | `components/PreviewDrawOverlay.tsx` [S] | P2 |
| File version history | Version list by source (AI, manual, restore); view, restore | `apps/daemon/src/project-file-versions.ts`, `FileViewer.tsx` [S] | P1 |
| Artifact lint | Regex lint for "AI slop" patterns (purple gradients, emoji as icons, made-up stats…), results returned to the agent | `apps/daemon/src/lint-artifact.ts` [S] | P2 |
| Post-run deliverable check | Checks whether the run actually created or touched an output file of the right type | `apps/daemon/src/run-deliverable-validation.ts` [S] | P1 |

### 1.3 Export

| Feature | Behaviour | Original app source | Level |
|---|---|---|---|
| ZIP | Download the whole project or a subfolder as a zip | `GET /api/projects/:id/archive` [S] | P0 |
| Standalone HTML | Pack a self-contained HTML file: CSS, images, fonts, modules all inlined as `data:` | `apps/daemon/src/artifacts/standalone-html.ts` [S] | P0 |
| PDF | Vector print, or raster capture of each slide/page then merge | `apps/desktop/src/main/pdf-export.ts`, `deck-export.ts` [S] | P1 |
| PNG/JPEG image | Capture one slide, the whole deck stacked vertically, or the full page | `apps/desktop/src/main/artifact-export.ts` [S] | P1 |
| PPTX (image) | One full-bleed image per slide | `apps/daemon/src/deck-export.ts` [S] | P1 |
| PPTX (editable) | Convert slide DOM into native shapes and text | `apps/desktop/vendor/dom-to-pptx` [S] | P2 |
| HyperFrames MP4 | Seek the timeline frame by frame, capture, stitch with ffmpeg | `apps/daemon/src/media/index.ts`, `apps/desktop/src/main/frame-capture.ts` [S] | P2 |
| Handoff | Open the folder in an editor, copy a prompt for a coding agent, generate `DESIGN-HANDOFF.md` | `components/HandoffButton.tsx`, `FinalizeDesignButton.tsx` [S] | P1 |

### 1.4 Content and extensions

| Feature | Behaviour | Original app source | Level |
|---|---|---|---|
| Skill library | List, view, import, delete user skills; a user skill overrides a bundled skill with the same id | `apps/daemon/src/skills.ts`; `docs/skills-protocol.md` [S][D] | P0 for read, P1 for CRUD |
| Design template | Catalogue of renderable templates (prototype, deck…) | `/api/design-templates` [S] | P0 |
| Design system | Browse about 150 packages; view the kit (colours, type, logo, voice); set default | `components/DesignSystemsTab.tsx`, `apps/daemon/src/design-systems/` [S] | P0 for read and pick, P1 for the browse page |
| Create design system | From a screenshot, URL, GitHub repo or folder; the agent writes `DESIGN.md` | `POST /api/design-systems/import/*`, `generation-jobs` [S] | P2 |
| Craft rules | Shared design rules, inserted into the prompt when the skill or system asks | `apps/daemon/src/craft.ts`, `craft/` [S] | P0 |
| Plugins and marketplace | Plugin page, "Try it", input form, install from registry, trust/capability | `routes/plugins/*`, `docs/plugins-spec.md` [S][D] | P2 (reading bundled plugins as presets only: P1) |
| Atoms and pipeline | Pipeline steps (critique, todo, file…) declared in a plugin | `apps/daemon/src/plugins/atoms.ts`, `pipeline-runner.ts` [S] | P2 |
| Media prompt templates | Library of image and video prompts (JSON) | `prompt-templates/`, `media/prompt-templates.ts` [S] | P2 |

### 1.5 Agents, settings, integrations

| Feature | Behaviour | Original app source | Level |
|---|---|---|---|
| Agent detection | Scan installed CLIs, probe version, model, auth status | `apps/daemon/src/runtimes/detection.ts` [S] | P0 |
| Claude Code runtime | Spawn `claude` stream-json, parse stream, resume, steer | `runtimes/defs/claude.ts`, `claude-stream.ts` [S] | P0 |
| Codex runtime | Spawn `codex` (exec-json or app-server) | `runtimes/defs/codex.ts` [S] | P0 for exec-json, P1 for app-server |
| Other runtimes (OpenCode, Cursor, Copilot, Qoder, Pi, dsh, plain text…) | One argv set and parser per CLI | `runtimes/defs/*.ts` [S] | P1 for OpenCode and Cursor; P2 for the rest |
| ACP family (Devin, Hermes, Kimi, Kiro, Kilo, Vibe, Trae, Reasonix) | JSON-RPC over stdio per Agent Client Protocol | `agent-protocol/acp/session.ts`; `docs/new-agent-runtime-acp.md` [S][D] | P1 (one shared ACP client covers 8 agents) |
| BYOK (API key) | No CLI: use the user's key; call through OpenCode or a direct streaming proxy | `runtimes/byok-opencode.ts`, `routes/chat.ts` (`/api/proxy/*`) [S] | P1 |
| Onboarding | First run: pick a local agent or API key, test the connection, allow skipping | `EntryShell.tsx` (`OnboardingView`) [S] | P0 (drop the cloud step) |
| Settings | Pick agent, model, reasoning; BYOK keys; MCP; media provider; language; custom instructions | `components/SettingsDialog.tsx` [S] | P0 for agent and model; P1 for the rest |
| MCP server for external agents | `od mcp` over stdio; other agents read and write project files | `apps/daemon/src/mcp.ts` [S] | P1 |
| `od mcp install <agent>` | Write MCP config into 17 other agents | `apps/daemon/src/mcp-agent-install.ts` [S] | P1 |
| MCP client | Daemon connects to the user's MCP servers (stdio, SSE, HTTP, OAuth 2.1) and passes them to the agent at spawn | `mcp-config.ts`, `mcp-oauth.ts` [S] | P2 |
| `od` CLI | Every feature has a subcommand calling the same HTTP API | `apps/daemon/src/cli.ts` [S]; `AGENTS.md` dual-track [D] | P0 for project, files, run, export; P1 for the rest |
| Media gen (image, video, audio) | Call providers with the user's key, write files into the project | `apps/daemon/src/media/` [S] | P2 |
| Live artifact | HTML page from template plus data, manual refresh from source | `apps/daemon/src/live-artifacts/` [S] | P2 |
| Automations, routines | Scheduled recurring prompts | `routines.ts`, `components/TasksView.tsx` [S] | P2 |
| Memory | Persist user preferences and rules across sessions | `memory*.ts`, `components/MemorySection.tsx` [S] | P2 |
| Connectors (Composio) | Connect external services via Composio | `apps/daemon/src/connectors/` [S] | P2 (depends on a third-party service, consider DROP) |
| Critique theater | Multi-role panel scores the design in rounds | `components/Theater/*`, `apps/daemon/src/critique/` [S]; `docs/critique-theater.md` [D] | P2 |
| Terminal | Terminal tab backed by a PTY, reconnect via Last-Event-ID | `apps/daemon/src/terminals.ts`, `TerminalViewer.tsx` [S] | P2 |
| Figma import, sketch, library, clipper, pets, what's new | Minor utilities | `FigmaImportModal.tsx`, `SketchEditor.tsx`, `LibrarySection.tsx`, `components/pet/*` [S] | P2 (pets, what's new: consider DROP) |
| i18n | 19 locales, with RTL | `apps/web/src/i18n/` [S] | P1 (en and vi first) |
| Theme | Light theme only, pick accent colour | `state/appearance.ts` [S] | P1 |
| Desktop shell | Main window, native menu, folder picker dialog, open external links | `apps/desktop/src/main/*` [S] | P0 |
| Auto-update | Update channels, checksum verification | `apps/desktop/src/main/updater*` [S] | P2 |

### 1.6 Dropped entirely (not ported)

| Group | Original app source |
|---|---|
| Sign-in, accounts, AMR Cloud, Vela, wallet, billing, paid plans, campaigns | Daemon: `routes/vela.ts`, `integrations/vela*.ts`, `runtimes/defs/amr.ts`, `runtimes/amr-*`, `media/vela*.ts`, `storage/amr-*`. Web: `components/Amr*`, `CloudSignInTip.tsx`, `CodingPlanUsage.tsx`, `PlanBadge.tsx`, `GoPlanSunsetDialog.tsx`, `campaigns/*`, `runtime/amr-*.ts`, MCP tool `start_vela_login`, CLI `od amr` [S] |
| Team, share, collab, invite, workspace, public publish | Daemon: `collab/` (109 files, about 25k lines), `routes/collab-*.ts`, `routes/team-*.ts`, `routes/social-share.ts`, `design-systems/{team-*,workspace-*}.ts`. Web: `collab/*`, `components/share/*`, `InviteDialog.tsx`, team routes. Desktop: `invite-deeplink*.ts`. CLI `od collab`, `od workspace`, `od share` [S] |
| Deploy (Cloudflare Pages, Vercel) | `deploy.ts`, `deploy/`, `routes/deploy.ts`, `deployments` table [S] |
| Telemetry, analytics, Langfuse, OTLP, surveys, diagnostics upload | Daemon: `analytics.ts`, `langfuse-*.ts`, `observability/`, `routes/telemetry.ts`, `storage/telemetry-outbox.ts`, `diagnostics-*.ts`, `services/*diagnostic*`. Web: `analytics/*`, `observability/*`, `PrivacyConsentModal.tsx`, `ExperienceSurvey.tsx`. Desktop: `observability.ts`, `crash-diagnostics.ts` [S] |
| GitHub stars badge, Discord presence, public metadata | `GithubStarBadge.tsx`, `useDiscordPresence.ts`, `routes/open-design-public-metadata.ts` [S] |
| Beta, prerelease, preview, stable release channels; Feishu; release CI | `tools/release`, `tools/pack` (channel part) [D] |
| "OD Next" rollout strategy and prompt A/B | `strategies/od-next/rollout*.ts` [S]. Rust keeps only **one** prompt set (see section 2.5) |

---

## 2. Current architecture

### 2.1 Processes and roles

```
Electron shell ──(sidecar IPC)── daemon (Express, Node 24) ── spawn CLI agent (cwd = project directory)
     │                                   │
     └─ renderer: web Next.js ─HTTP+SSE──┘  SQLite + files on disk
```

- The **daemon** is the single source of truth for `/api/*`, about 400 routes [S `apps/daemon/src/server.ts`, `routes/**`]. The daemon handles:
  - storing projects, conversations, messages;
  - files, versions, preview, import, export;
  - detecting, spawning, streaming, cancelling agents;
  - prompt assembly;
  - plugins, MCP, media, memory.
- **Web** is an SPA. Next.js only rewrites `/api/*`, `/artifacts/*`, `/frames/*` to the daemon. In the packaged build, the daemon serves the static export `apps/web/out` itself [D `docs/architecture.md` §1–2].
- **Electron** handles: windows, native dialogs, the `od://` protocol, auto-update, and everything that needs a browser engine for export (PDF, slide capture, frame capture). The daemon calls Electron over IPC with 4 actions: `export-pdf`, `export-artifact`, `render-slides`, `render-frames` [S `packages/sidecar-proto/src/index.ts:104–110`].
- **The `od` CLI** is the daemon binary itself: `od` with no subcommand starts the daemon. Default port 7456, host `127.0.0.1` [S `cli.ts:9885`, `daemon-startup.ts`].

### 2.2 Web ↔ daemon protocol

- **HTTP and SSE only, no WebSocket**: no `ws` dependency, no upgrade handler [S].
- **SSE format**: each event is `id: N` / `event: <name>` / `data: <json>`. Headers `no-cache, no-transform` and `X-Accel-Buffering: no`. A keepalive comment every 25 s [S `server.ts:3082–3095`].
- **Run SSE event names**: `start`, `run_retry_attempted`, `agent`, `stdout`, `stderr`, `diagnostic`, `error`, `end` [S `packages/contracts/src/sse/chat.ts:510`].
  - `start` carries runId, agentId, model, `protocolVersion: 1`.
  - `end` carries `status` (succeeded, failed, canceled), exit code, artifact file list, classified error, `resumable` and `retryable` flags.
- **`agent` payload** (field `type`), a normalised union across all CLIs [S `sse/chat.ts:171–446`]:

  | Group | Type |
  |---|---|
  | Status | `status` (label, model, sessionId, ttftMs) |
  | Text, thinking | `text_delta`, `thinking_start`, `thinking_delta`, `thinking_tokens` |
  | Tool | `tool_use` (id, name, input), `tool_result` (toolUseId, content, isError), `tool_input_delta`/`_target`/`_progress`, `tool_in_flight` (live only, not persisted) |
  | Cost | `usage` (token, cost, stopReason), `request_usage` |
  | Daemon-generated | `conversation_title`, `next_steps`, `artifact_focus`, `artifact` (for the plain runtime), `live_artifact*` |
  | Guards | `tool_loop`, `fabricated_role_marker`, `raw` (unparseable line, length-capped) |

- **Replay and reconnect** [S `runtimes/runs.ts:727, 1485–2000`]:
  - Each run has increasing event ids. A ring buffer of 2,000 events in RAM.
  - Every event is also appended to `runs/<runId>/events.jsonl`. `start`, `error`, `end` are written synchronously to `runs/<runId>/state.json`.
  - The client sends `Last-Event-ID` or `?after=N`. Anything older than the ring is re-read from jsonl; live events arriving during replay are queued and de-duplicated by id. For a finished run, the whole history is sent and the stream closes.
  - On daemon restart: finished runs are reloaded from disk; runs that were mid-flight get an extra `error` plus `end{failed}`.
  - **Criterion for the Rust version**: disconnect at event k, reconnect with `Last-Event-ID: k`, must receive exactly from k+1, no duplicates, no gaps.
- **Main P0 endpoints** [S `routes/project/index.ts`, `routes/runs.ts`, `import-export-routes.ts`]:

  | Group | Endpoint |
  |---|---|
  | Health | `GET /api/health`, `/api/version` |
  | Agent | `GET /api/agents` (has a mode that streams detection results) |
  | Content | `GET /api/skills`, `/api/design-templates`, `/api/design-systems[/:id]`, `/api/craft` |
  | Project | `GET/POST /api/projects`, `GET/PATCH/DELETE /api/projects/:id`, `POST .../duplicate` |
  | File | `GET/POST .../files`, `GET .../files/<path>`, `POST .../files/rename`, `DELETE .../files/<path>`, `POST .../upload` |
  | Preview | `GET .../raw/<path>` (Range, ETag), `GET .../preview-url`, `GET .../preview/<scope>/<path>` |
  | Version | `GET/POST .../files/<path>/versions`, `POST .../versions/:vid/restore` |
  | Conversation | `GET/POST .../conversations`, `GET .../conversations/:cid/messages`, `PUT .../messages/:mid` |
  | Run | `POST /api/runs` → 202 `{runId…}` (idempotent on `clientRequestId`); `POST /api/chat` (creates a run and streams in the response); `GET /api/runs/:id/events` (SSE); `POST /api/runs/:id/cancel`; `POST /api/runs/:id/steer` |
  | Project events | `GET /api/projects/:id/events` (SSE: `ready`, `file-changed {path, kind: add/change/unlink}`, `conversation-created`) |
  | Export | `POST .../export/{html,pdf,pdf-image,image,pptx}`, `GET .../archive` |
  | Config | `GET/PUT /api/app-config`, `GET/PUT /api/media/config` |
  | Active project | `POST/GET /api/active` |

- **`POST /api/runs` body** (`ChatRequest`, main fields) [S `packages/contracts/src/api/chat.ts:116`]: agentId, message, projectId, conversationId, message ids (client-generated), clientRequestId, skillId, skillIds, designSystemId, attachments, commentAttachments, model, reasoning, sessionMode, locale.
- **Security** [S `server.ts:3226–3557`, `api-token-auth.ts`, `origin-validation.ts`]:
  - Binds to loopback by default, no auth needed.
  - Binding beyond loopback requires `OD_API_TOKEN` (Bearer or Basic).
  - Origin guard: requests without `Origin` (CLI) pass. `Origin: null` may only GET preview files. Any other origin must be loopback or LAN on the right port, or listed in `OD_ALLOWED_ORIGINS`.
  - The agent calls back into the daemon with `OD_TOOL_TOKEN`: TTL at least 15 minutes, valid for one run only, restricted to an allowlist of tool endpoints [S `tool-tokens.ts`].
  - **The Rust version should keep**: loopback by default, origin guard, per-run tool token. **Drop**: the desktop HMAC for folder import, since the Rust shell calls it in-process.

### 2.3 Launching and streaming coding agents

**General model** [S `runtimes/types.ts:145–338`, `runtimes/registry.ts`; D `docs/agent-adapters.md`]:
- Each agent is **a data declaration**, with no agent-specific `run()` code. The declaration contains:
  - the binary and fallback binaries;
  - a function building argv from prompt, images, extra dirs, model, reasoning, session;
  - `streamFormat` and its parser;
  - how the prompt is fed: stdin, file or argv; `text` or `stream-json`;
  - resume flags;
  - timeouts;
  - version, help, model, auth probes;
  - how MCP config is injected.
- One shared engine handles detection, spawn, parse, cancel.
- **The Rust version should keep exactly this model**: a pure-data `RuntimeDef` trait plus one `RunEngine`.

**Agent detection** [S `runtimes/detection.ts`]:
- Resolve the exact binary path the run will spawn, then run the version probe.
  - No binary or not executable: unavailable.
  - Runs but rejects the version flag: still available.
- Then run the help probe (look for flag strings in `--help`), model probe and auth probe in parallel. Each probe is isolated; one broken CLI does not empty the list. Nothing is cached to disk.

**Spawn** [S `server.ts:14864–15093`, `runtimes/agent-process.ts`, `runtimes/env.ts`]:
- cwd is the project directory.
- POSIX: the agent leads its own process group (detached) so the whole tree can be killed.
- Runtimes that take the whole prompt at once get it via stdin pointed at a file (all-or-nothing). Claude stream-json, ACP, pi, dsh and codex app-server keep the pipe open.
- A process record is written to `runs/<runId>`. On the next start, the daemon cleans up orphaned agents.
- **Env passed to the agent**:
  - `OD_DATA_DIR`, `OD_DAEMON_URL`, `OD_TOOL_TOKEN`, `OD_PROJECT_ID`, `OD_PROJECT_DIR`, `OD_BIN` (path to `od` so the agent can call tools);
  - Claude only: `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`;
  - `OD_API_TOKEN` is never passed.
- **Skill staging**: the active skill is copied into `<cwd>/.od-skills/<name>-<hash>/`, no symlinks. The SKILL.md body gets a preamble pointing to the auxiliary files [S `skills.ts` `withSkillRootPreamble`; D `docs/skills-protocol.md` §3].

**Claude Code** [S `runtimes/defs/claude.ts:67–117`, `run-steering.ts:95–103`, `claude-stream.ts`]:
- **argv**: `-p --input-format stream-json --output-format stream-json --verbose`, plus conditionally:
  - `--include-partial-messages`, if `-p --help` lists the flag;
  - `--model <m>`, unless the model is `default`;
  - `--add-dir <dirs>`;
  - `--resume <id>` when resuming, otherwise `--session-id <new uuid>`;
  - always `--permission-mode bypassPermissions`. The rewriter must **decide whether to keep it**: this is a safety choice, see section 5.6.
- **Prompt**: one JSONL line shaped as a user message (role user, content is one text block), written to stdin, **stdin kept open**.
- **MCP**: write `.mcp.json` into cwd, only when cwd is a daemon-managed project directory.
- **Steer**: write one more user-message line in the same format, only while the run is alive and stdin is open. Errors return 409 with reason `runtime_unsupported`, `run_terminal` or `stdin_closed`.
- **Turn boundary and closing stdin**: read `stop_reason` in priority order:
  1. `stream_event` → `message_delta.delta.stop_reason` (newer CLI, needs `--include-partial-messages`);
  2. `assistant.message.stop_reason` (older);
  3. the end-of-turn `result` frame.
  - De-duplicate by message id. Skip sub-agent frames (those with `parent_tool_use_id`).
  - A `stop_reason` other than `tool_use` ends the turn and closes stdin. Closing stdin on `tool_use` truncates the answer [D `AGENTS.md` Agent runtime conventions].
- **Mapping Claude frames to standard events**: `system/init` → `status`; `content_block_delta` → `text_delta`/`thinking_delta`/`tool_input_delta`; `content_block_stop` → `tool_use`; `tool_result` blocks in a `user` frame → `tool_result`; `result` → `usage`, plus `error` if `is_error`. `TaskCreate`/`TaskUpdate` calls are merged into one `TodoWrite` snapshot [S `claude-stream.ts:304–345, 717–1160`].
- **Test fixtures**: the original app has real stream recordings in `apps/daemon/tests/fixtures/claude-cli-recordings/` [D]. This is Apache-2.0 data, usable as golden input (needs NOTICE).

**Codex** [S `runtimes/defs/codex.ts:327–688`]:
- **exec-json** (do first):
  - `exec --json --skip-git-repo-check --sandbox workspace-write`;
  - `-c` options: enable network for the sandbox, `model_reasoning_summary`, enable `update_plan`;
  - `-C <cwd>`, `--add-dir`, `--model`;
  - prompt via stdin.
  - Resume: `exec resume --json … <thread_id>`.
- **app-server** (default in the original app): JSON-RPC over stdio (`initialize`, `thread/start|resume`, `turn/start`, `turn/interrupt`) [S `agent-protocol/codex-app-server/session.ts`].
- **Mapping** [S `json-event-stream.ts:1161–1441`]:

  | Codex frame | Standard event |
  |---|---|
  | `thread.started` | `status{sessionId}` |
  | `command_execution` | `tool_use` named Bash + `tool_result` (error if exit ≠ 0) |
  | `file_change` | `tool_use` Write/Edit per file |
  | `todo_list` | `TodoWrite` |
  | `agent_message` | `text_delta` |
  | reasoning | `thinking_delta` |
  | `turn.completed.usage` | `usage` |

**Other runtimes, summary** [S `runtimes/defs/*.ts`]:

| Runtime | Command | Prompt | Parser |
|---|---|---|---|
| Cursor Agent | `cursor-agent --print --output-format stream-json --stream-partial-output --force` | stdin | json-event-stream (cursor) |
| OpenCode | `opencode run --format json [-s session] [-m provider/model]`, MCP config via env | stdin | json-event-stream (opencode) |
| Copilot | `copilot --allow-all-tools --output-format json` | stdin | custom |
| ACP family | `<bin> acp` or equivalent; JSON-RPC `initialize` → `session/new` or `session/load` → `session/prompt`; auto-approves `session/request_permission` | RPC | shared ACP |
| DeepSeek Harness (dsh) | `dsh --profile <name> --stdio`; custom JSONL protocol (`execute`, `cancel` / `text`, `tool_call`, `usage`, `result`) | RPC | dsh-profile |
| Plain text (Qwen, Aider…) | stdout is raw text; at end of run `<artifact>` blocks are split into files | argv, stdin or file | plain |

- **Suggested port order**: Claude → Codex exec-json → shared ACP → OpenCode → the rest.

**Cancel** [S `runtimes/runs.ts:2228–2526`]:
- If the protocol has an abort command (ACP `session/cancel`, dsh `cancel`), send it first and wait 3 s.
- Then close stdin, send SIGTERM to **the whole process group**, wait 3 s, then SIGKILL the group.
- Windows: snapshot the process tree, then escalate in the same order.
- Signals are tied to that attempt's exact child and pgid, so a retry is never killed by mistake.

**Resume** [S `agent-session-resume.ts:121–436`]:
- Table `agent_sessions`, keyed by (conversation, agent), stores sessionId, model, cwd, last message, hash of the stable prompt prefix.
- No resume (resend the full transcript) when the model changed, cwd changed, the pointer is missing, or the conversation has moved on.
- Two ways to get the session id:
  - Claude: the daemon generates a UUID and passes it via `--session-id`;
  - Codex, OpenCode, ACP: captured from the stream.
- A failed resume (detected via stderr, or via an error `result` with 0 turns) clears the handle and reruns once automatically, without reporting an error to the user.

**Retry, timeout, guards** [S `run-retry-policy.ts`, `runtimes/chat-run-lifecycle.ts`, `tool-loop-guard.ts`]:
- **Retry**: at most once, only for transient errors and **only if nothing has been shown to the user yet**. Backoff 500 ms (1,000 ms when rate limited), doubling up to 8 s, with jitter.
- **Timeout**: stop after 10 minutes with no output (Copilot 30 minutes).
- **Tool-loop guard**: warn after 5 consecutive errors, stop at 10. The same action failing 4 times warns, 8 stops. Default is warn only.

### 2.4 Prompt assembly

- The whole assembled prompt is **one user message**; the CLI's system-prompt flags are not used [S `runtimes/chat-prompt-inputs.ts:115–307`].
- **Two blocks**: the instructions (read first), then the user's request, with attachments and images.
- **Order of blocks in the instructions** (legacy version) [S `apps/daemon/src/prompts/system.ts:1040–1528`]:
  1. anti prompt injection;
  2. mode override (plan, chat, API);
  3. locale;
  4. discovery: guidance to ask back via question-form;
  5. visual direction library, dropped when a design system is set;
  6. device frame and platform contract;
  7. active design system: `DESIGN.md`, `tokens.css`;
  8. craft;
  9. active skill;
  10. plugin;
  11. deck framework;
  12. media contract;
  13. critique;
  14. comment identity.
  - Per-turn blocks go last, so the head stays stable for the prompt cache.
- **The design system has its own order too** [D `docs/skills-protocol.md` §5]: `USAGE.md` → `DESIGN.md` → import-mode guidance → `tokens.css` → component manifest → index of rich-content files (the agent reads them when needed) → craft → skill body.
- **Two parallel implementations** (legacy and "OD Next"), chosen per run [D `docs/prompt-composition.md`; `AGENTS.md` Prompt variants]. The Rust version **does only one**. Proposal: follow legacy, since its structure is clear and it lives entirely in `apps/daemon/src/prompts/`.
- **Two execution profiles** [D `docs/architecture.md` §4]:
  - Runtimes with file tools (Claude, Codex…): the agent writes files into the project itself; the preview follows the files.
  - Plain or BYOK runtimes without tools: the model returns **one** complete `<artifact>` block; the host splits it out and writes it as a file.

### 2.5 MCP server `od mcp`

- **Transport**: stdio only. The server is stateless; every tool is one HTTP call to the daemon. Tools can still be listed while the daemon is down. Exits on its own after 30 minutes idle [S `apps/daemon/src/mcp.ts`, `mcp-bootstrap.ts`].
- **Finding the daemon**: `--daemon-url` → `OD_DAEMON_URL` → sidecar state → default `http://127.0.0.1:7456`.
- **Tools** (22; 20 once the 2 login tools are dropped) [S `mcp.ts:365–860`]:
  - read: `list_projects`, `get_project`, `get_active_context`, `get_artifact`, `get_file` (by line, default 2,000), `search_files` (literal, case-insensitive, default 200 results), `list_files`;
  - write: `create_artifact` (refuses if the file exists), `write_file`, `delete_file`, `delete_project` (requires id and `confirm: true`), `create_project`;
  - catalogue: `list_skills`, `list_plugins`, `list_agents`;
  - run: `start_run`, `get_run`, `cancel_run`;
  - brief: `collect_brief` and `confirm_brief`, open the brief form via MCP Apps.
- **Resources**: `od://focus/active`, `od://skills/<id>/SKILL.md`, `od://design-systems/<id>/DESIGN.md`.
- **Active Project** [S `routes/active-context.ts`, `constants.ts`]:
  - Web POSTs `/api/active` on every route change. Kept in RAM only, TTL 5 minutes.
  - An MCP tool with `project` left empty uses the active project; the response states that the active context was used.
- **Second MCP server** `od mcp live-artifacts`: create, list, update, refresh live artifacts and connectors [S `mcp-live-artifacts-server.ts`].
- **`od mcp install <agent>`**: 17 agents, 3 install methods [S `mcp-agent-install.ts`]:

  | Method | Agents |
  |---|---|
  | Call that CLI's own `mcp add` command | claude, codex, kimi, reasonix |
  | Deep-merge one entry into a JSON config file | cursor, copilot, cline, opencode, openclaw, antigravity, kiro, trae, raven, claude-desktop |
  | Print a snippet only | pi, hermes, vibe |

  Supports `--print` (dry run) and `--uninstall`.

### 2.6 The `od` CLI

- Dispatch via `SUBCOMMAND_MAP`. Most subcommands are thin clients calling `/api/*`, with `--json` and `--daemon-url` [S `cli.ts:404`; D `AGENTS.md` dual-track].
- **Groups to keep**:

  | Group | Subcommand |
  |---|---|
  | Project | `project` (create, list, info, delete, duplicate, import-folder), `conversation` |
  | Run | `run` (start, watch as NDJSON, cancel, steer, continue, list, info) |
  | File | `files` (list, read, write, upload, delete, diff, versions…) |
  | Export, checks | `export`, `lint` |
  | Content | `skills`, `design-systems`, `craft`, `templates`, `plugin` (list, info, apply, validate) |
  | Media | `media` (generate, scaffold, wait) |
  | MCP | `mcp`, `mcp install` |
  | Daemon | `daemon` (start, status, stop), `status`, `version`, `doctor`, `config` |
  | Agent-invoked | `od tools …` (live-artifacts, design-systems read, deliverable-syntax), uses the tool token |

- **Dropped groups**: `amr`, `collab`, `workspace`, `share`, `deploy`, `strategy rollout`, `diagnostics export`, and the `plugin` login, publish, yank commands.
- **Exit codes to keep**: 66 when a plugin lacks a capability [D `docs/plugins-spec.md` §9.1]; 1 when `od lint` finds errors at the `--fail-on` threshold.

---

## 3. Data formats

### 3.1 Data directory

- **Data root** = `OD_DATA_DIR`, resolved once at startup. Every data path derives from this root [D `AGENTS.md` Daemon data directory contract].
- **Under the root** [S `server.ts:1420–1491`]:

  | Path | Contents |
  |---|---|
  | `app.sqlite` | Database (WAL mode) |
  | `app-config.json`, `media-config.json` | App config, media config |
  | `projects/<projectId>/` | Daemon-managed projects |
  | `runs/<runId>/state.json`, `events.jsonl` | Run state and event log |
  | `artifacts/<stamp>-<slug>/index.html` | Standalone saved artifacts |
  | `chat-artifact-blobs/objects/aa/bb/<sha256>` | Content-addressed store for chat card snapshots |
  | `skills/`, `design-templates/`, `design-systems/`, `plugins/` | User-added versions; override bundled ones with the same id |
  | `mcp-config.json`, `mcp-tokens.json`, `mcp-oauth-clients.json` | MCP config and credentials |

- **Read-only bundled content**: `skills/`, `design-systems/`, `design-templates/`, `craft/`, `prompt-templates/`, `plugins/_official`, `plugins/registry`, `assets/frames`. Located via `OD_RESOURCE_ROOT` [S `server.ts:1344–1399`].

### 3.2 Projects on disk

- **Metadata does not live in the project directory**: it is in SQLite (`projects.metadata_json`, type `ProjectMetadata`) [S `packages/contracts/src/api/projects.ts:195`]. Main fields:
  - `kind`: prototype, deck, template, image, video, audio, other;
  - `intent`, e.g. live-artifact;
  - platform, fidelity, `entryFile`;
  - `baseDir` (for folder imports), `importedFrom`, `linkedDirs`.
- **Inside `projects/<id>/`** [S `projects.ts`, `artifacts/manifest.ts`, `project-file-versions.ts`, `live-artifacts/store.ts`]:
  - User and agent files. Recursive listing, newest first; skips `node_modules`, hidden files and ignored directories.
  - `<file>.artifact.json`: artifact manifest for the entry file, hidden from the listing. Contains:
    - `kind`: html, deck, react-component, markdown-document, svg, diagram, code-snippet, mini-app, design-system;
    - `renderer`, `exports[]`, `status` (streaming, complete, error), `entry`, `title`, `supportingFiles`;
    - metadata up to 16 KB.
  - `.file-versions/<first 24 chars of sha256(file name)>/manifest.json` (schemaVersion 2: fileName, currentVersionId, entries, deletedAt) plus content files `NNNN-<id>.<ext>`.
  - `.live-artifacts/<id>/`: `artifact.json`, `template.html`, `data.json`, `index.html`, `provenance.json`, `refreshes.jsonl`, `snapshots/`.
  - `.od-skills/`: skills staged for each run.
  - `.transcript.jsonl`: transcript, generated on demand.
  - `.file-versions` and `.live-artifacts` are reserved names; the file API refuses to touch them.
- **Projects at an external location**: add `.open-design/project.json` (schemaVersion 1, id, name, timestamps, skillId, designSystemId) [S `project-locations.ts:8–22`].
  - **Proposal for the Rust version**: every project has this file (rename the hidden directory for the new repo), so a project can describe itself if SQLite is lost.
- **Folder import**: never creates the root directory; rejects any path with a hidden segment; every path goes through safe resolution [S `projects.ts:129`; D `docs/architecture.md` §6].

### 3.3 SQLite

- **Migrations**: `CREATE TABLE IF NOT EXISTS` plus `ALTER TABLE ADD COLUMN` after checking `PRAGMA table_info`. `user_version` is not used [S `db.ts:83–691`]. **The Rust version should use `user_version`** with numbered migrations.
- **Required core tables**:

  | Table | Contents |
  |---|---|
  | `projects` | id, name, skill_id, design_system_id, pending_prompt, metadata_json, custom_instructions, created_at, updated_at |
  | `conversations` | id, project_id (cascade), title, session_mode, timestamps |
  | `messages` | id, conversation_id, role, content, agent_id, run_id, run_status, last_run_event_id, position, timestamps; JSON: `events_json` (persisted events), attachments, comment_attachments, produced_files, feedback |
  | `message_event_batches` | Events written incrementally during a run; merged into `messages.events_json` when the run ends |
  | `agent_sessions` | Key (conversation_id, agent_id): session_id, stable_prompt_hash, model, cwd, last_message_id |
  | `templates` | Templates the user saved from a project |
  | `tabs`, `tabs_state` | Open tabs per project |
  | `preview_comments` | selector, position_json, note, status, slide_key, sort_key (author and cloud columns dropped) |
  | `media_tasks` | Media generation tasks |
  | `routines` | Schedules (P2) |
  | `installed_plugins`, `plugin_marketplaces`, `applied_plugin_snapshots` | Plugins (P2) |

- **Runs are not a table**: they live in RAM plus the `runs/<id>/` directory.
- **Dropped tables**: `deployments`, `workspace_*`, `team_*`, about 18 collab tables, `telemetry_*`, `incidents`, `amr_*`.

### 3.4 Skills and design templates

- **Format**: a directory containing `SKILL.md` (YAML frontmatter plus Markdown body), optionally `assets/`, `references/`, scripts, `example.html`, `examples/<key>.html` [D `docs/skills-protocol.md` §1–2].
- **Frontmatter keys**:
  - top level: `name`, `description`, `triggers`, translations of name and description;
  - under `od`: `mode`, `surface`, `platform`, `scenario`, `category`, `preview{type, entry}`, `design_system.requires` (default true), `craft.requires`, `example_prompt`, `featured`, `speaker_notes`, `animations`, `upstream`, `upstream_license`, `critique.policy`;
  - `inputs`, `parameters`, `outputs`, `capabilities_required` only matter for plugins.
- **The original app's frontmatter parser is a YAML subset**: scalars, `|` block strings, flat lists, indentation-nested maps; no flow style or anchors [S `design-systems/frontmatter.ts`]. If the Rust version uses a full YAML parser it **must be tested against every bundled file** to make sure results match.
- **Normalisation** [S `apps/daemon/src/skills.ts`]:
  - `mode` is one of 8 values: prototype, deck, template, design-system, image, video, audio, utility. If wrong or missing, it is inferred by regex over the description and body in the order image → video → audio → deck → design-system → template, else prototype.
  - `platform` defaults to desktop; becomes mobile if the text mentions mobile, phone, ios, android.
  - `example_prompt` defaults to the first sentence of `description`, at most 320 characters.
  - id is `name`, falling back to the directory name. There is an alias table for renamed ids.
  - Each `examples/<key>.html` becomes a child card with id `<parent>:<key>`.
  - Rescanned on every request; user root first, bundled root second, first match wins.
- **Classification** [D `specs/current/skills-and-design-templates.md`]: prototype, deck, template and media outputs are design templates (`/api/design-templates`). utility and design-system are functional skills (`/api/skills`).
- **Deck contract** [D `design-templates/AGENTS.md`; S `packages/contracts/src/prompts/deck-framework.ts`]:
  - Fixed 1920×1080 stage, scaled by `min(width/1920, height/1080)`.
  - Each slide is a `section.slide`; only the `.active` slide is shown.
  - Keys: →, ↓, PgDn, Space go forward; ←, ↑, PgUp go back; Home and End jump to the ends.
  - The current slide is remembered in localStorage.
  - When printing: `@page` 1920×1080 px, one slide per page.
  - `<html>` carries an attribute marking the deck protocol version.

### 3.5 Design systems

- **Package** [D `docs/design-systems.md` §1; S `design-systems/_schema/manifest.schema.ts`]:
  - Required: `manifest.json`, `DESIGN.md`, `tokens.css`.
  - Optional: `USAGE.md`, `components.html`, `components.manifest.json`, `design-tokens.json`, `tailwind-v4.css`, `preview/*.html`, `source/*`, `DESIGN-<lang>.md` (17 languages; the loader currently does not read these files [S]).
- **Manifest**:
  - `schemaVersion` of the form `…/v1`, `id` matching the directory name, `name`, `category`, `description`;
  - `source{type: bundled|local|github|shadcn, origin}`;
  - `files{…}`, `importMode` (normalized, hybrid, verbatim);
  - `craft{applies, suggested, exemptions}`, `preview{dir, pages[]}`.
- **`DESIGN.md`**: free prose, at least 7 H2 sections.
- **Tokens**, by layer [S `packages/contracts/src/design-systems/token-schema.ts`]:

  | Layer | Contents |
  |---|---|
  | A1 identity | background, surface, text, secondary colour, border, accent, display font, body font |
  | A1 structure | type scale xs–4xl, leading, tracking, container, section spacing |
  | A2 (with fallback values) | accent states, danger/warn/success, focus ring, mono font, radius, space 1–12, elevation, easing, motion durations |
  | B (optional) | auxiliary slots |

- **Fallback order when listing**: title comes from user metadata → manifest → H1 → frontmatter → directory name. Category, summary and swatches have similar fallback chains [S `design-systems/index.ts:~320`].

### 3.6 Craft, prompt templates, plugins

- **Craft**: 11 files `craft/<slug>.md`. Slugs to load = the skill's `craft.requires` ∪ the design system's `craft.applies` ∪ `typography` (for decks), minus `exemptions`. Each file becomes a `### <slug>` section, joined by horizontal rules. Missing files are skipped silently [S `craft.ts`].
- **Prompt templates**: `prompt-templates/{image,video}/*.json` with fields id, surface, title, summary, category, tags, model, aspect, prompt, preview image, source (with `license`) [S `media/prompt-templates.ts`].
- **Plugins** [D `docs/plugins-spec.md`; schema `docs/schemas/open-design.plugin.v1.json`]:
  - A directory with `SKILL.md` (or `.claude-plugin/plugin.json`), plus an optional `open-design.json`.
  - Top-level fields: name, title, version, description, license, tags, compat.
  - `od` block: `kind` (skill, scenario, atom, bundle), mode, `preview`, `useCase.query` (with `{{…}}` variables), `context{skills, designSystem, craft, mcp, atoms}`, `pipeline.stages[{atoms, repeat, until}]`, `genui.surfaces`, `connectors`, `inputs`, `capabilities`.
  - Capabilities: `prompt:inject`, `fs:read`, `fs:write`, `mcp`, `subprocess`, `bash`, `network`, `connector[:id]`. Trust: restricted or trusted. A missing capability yields HTTP 409 or exit 66.
  - Marketplace: `plugins/registry/*/open-design-marketplace.json`, about 420 entries pointing to GitHub.

### 3.7 Reusing content under its licenses

- The original repo is Apache-2.0, **with no NOTICE file at the root** [S]. But the content directories hold **102 separate LICENSE files**, mostly MIT; some prompts are CC-BY-4.0; some fonts ship with a license and some without [S, counted with grep].
- **Recommendation**: the target repo creates a `NOTICE` crediting the original app, and **keeps every nested LICENSE file intact** when copying.

| Directory | Count | Reusable as-is? | Notes |
|---|---|---|---|
| `craft/` | 11 files | Yes | Adapted from MIT sources (noted in `craft/README.md`); keep the README |
| `design-templates/` | about 115 directories | Yes, keep nested LICENSEs | 37 third-party MIT LICENSE files (many deck sets); `hyperframes/` is forked from an Apache-2.0 source |
| `skills/` | about 170 directories | Yes, keep nested LICENSEs | 36 MIT LICENSEs. 194 skills declare `upstream` but only 32 declare `upstream_license`: Atlat audits the rest before release |
| `design-systems/` | about 152 packages + `_schema` | Yes license-wise, **trademark risk** | Many packages are named after real brands. Rename the display names or ship only a neutral set, and ask the owner. Attribution is in `design-systems/README.md` |
| `prompt-templates/` | 107 JSON | **Conditional** | 72 files CC-BY-4.0 (attribution required), 31 Apache, 1 MIT, 2 files with unclear license (exclude). Preview images are third-party CDN URLs |
| `plugins/_official/atoms`, `scenarios` | 13 + 14 | Yes | Drop cloud and rollout scenarios (take `od-next-strategy` only if the Next prompt is chosen) |
| `plugins/_official/design-systems` | 143 | Not needed | Byte-identical copy of `design-systems/`; manifest says MIT, inconsistent with the root Apache |
| `plugins/_official/examples` | 188 | Yes, keep nested LICENSEs | Ships OFL fonts; some fonts **have no license file** (`examples/open-design-homepage/assets/fonts`): exclude |
| `plugins/_official/image-templates`, `video-templates` | 45 + 63 | Conditional | Mostly CC-BY-4.0. Hashed fonts in `video-templates/*/source/fonts` have no license: exclude or audit |
| `plugins/community/` | 10 | Audit each | |
| `templates/` | 10 files | Yes | The original app's runtime does not load this directory |
| `apps/daemon/tests/fixtures/claude-cli-recordings/` | | Yes (test data) | Use as golden tests for the parser |

---

## 4. Preview and export

### 4.1 HTML preview

**Renderer chosen by manifest** [S `apps/web/src/artifacts/renderer-registry.ts`]: react-component → deck-html → html → markdown → svg. Other files go by file type (section 1.2).

**Two load modes** [S `components/file-viewer-render-mode.ts:104–137`]:
- **By URL** (default for multi-file artifacts): iframe `src` = `/api/projects/:id/raw/<path>?v=<mtime>&…`. Each asset loads separately and is cacheable.
- **srcDoc**: the host builds a document with the bridge pre-injected. srcDoc is required when:
  - it is a deck;
  - inspect, edit, palette or tweaks is on;
  - there is a form or a hash router;
  - the HTML references files with paths starting at `/`;
  - comment or draw is on and the URL has no matching bridge.
- **Selection heuristic**: raw string matching, no HTML parsing. E.g. `localStorage` present means a Web Storage shim is needed; `SharedArrayBuffer`, `.wasm`, WebGL2, `navigator.gpu` mean a "powered preview" (cross-origin isolated) is needed.
- The host keeps both iframes at once and only toggles CSS visibility, so switching modes does not flicker [D `AGENTS.md` Chat UI].

**Iframe sandbox** [S `FileViewer.tsx:437–448`]:
- Normal: `allow-scripts allow-forms allow-downloads`, opaque origin.
- Powered: adds `allow-same-origin` and runs on a per-project host (`od-<hash>.localhost`).

**Daemon preview serving** [S `routes/project/index.ts:5950, 6454, 6846, 7014, 7093, 7233`]:
- `raw`: ETag and Last-Modified. For HTML ≤ 2 MiB, if the query asks for a bridge, the bridge script is injected before the author's scripts, along with a `<base href>` pointing at the preview scope.
- `preview-url`: issues a short-lived scope token with CSP and sandbox. `preview/<scope>/…` is served with a strict CSP: only `'self'`, `data:`, `blob:`; `connect-src 'none'`; `form-action 'none'`; `base-uri 'none'`.
- `powered`: `Document-Isolation-Policy` header, no CSP.

**Bridge**: a script injected into the page, talking to the host via `postMessage`, with types prefixed `od:` [S `runtime/srcdoc.ts`; `packages/contracts/src/runtime/deck-protocol.ts`]:

| Bridge | Job |
|---|---|
| Deck | Control next, prev, first, last, go to slide i; report state (active slide, total); report deck ready; report Esc |
| Selection | Hover and select elements for comment and inspect; identify elements by pre-assigned `data-od-id` |
| Inspect | Set, reset, replay, extract style overrides |
| Palette, tweaks, edit | Recolour, show the parameter panel, edit text |
| Snapshot | Capture **without a real screenshot**: clone the DOM, copy computed styles, draw via SVG foreignObject |
| Export-capture | Wait for fonts, images and 2 animation frames; walk each slide |
| Guard | Anti redirect loop (always on), block focus stealing, Web Storage shim, measure content size |

**Live reload** [S `project-watchers.ts`, `providers/project-events.ts`]:
- One watcher per project, reference-counted by subscribers; created on the first subscriber, closed when the last one leaves.
- Waits for the file to settle for 200 ms. Ignores symlinks and junk files. Falls back to polling when watch handles run out.
- Events go over the project SSE. The client reconnects with 1–30 s backoff; on an event it refreshes the file list, and the changed `?v=` makes the iframe reload.
- **Criterion for the Rust version**: file write → preview updates in ≤ 500 ms (excluding webview render time).

### 4.2 Export: what needs a browser engine

| Capability | Needs browser engine? | Why | Original app uses |
|---|---|---|---|
| Interactive preview, bridge | Yes | Runs the artifact's JS and layout | Iframe in the renderer |
| File serving, CSP, bridge injection, `<base>` | No | Header and string rewriting | Express |
| Live reload | No | Watcher plus SSE | chokidar |
| Standalone HTML | No | Walk the dependency graph, inline as `data:`, modules into an importmap. Limits: entry 2 MiB, 2,500 nodes, 50 MiB per asset, output 100 MiB; over that returns 413 | cheerio, postcss, babel [S `artifacts/standalone-html.ts`] |
| ZIP | No | Streamed archive | [S] |
| Transcript JSONL | No | Read SQLite, write a temp file then rename, with a lock | [S `transcript-export.ts`] |
| Vector PDF | Yes | `@page`, print layout | Electron `printToPDF`; deck page 13.333×7.5 in [S `desktop/src/main/pdf-export.ts`] |
| Raster PDF (default, CLI included) | Yes | Capture each slide then merge; the original app chose this because `printToPDF` loses CJK glyphs | CDP `Page.captureScreenshot` + pdf-lib [S `export-cli-routing.ts`, `deck-export.ts`] |
| Image PPTX | Yes (for capture) | One full-bleed image per slide, 13.333 in wide | pptxgenjs [S] |
| Editable PPTX | Yes | Measures real DOM layout | dom-to-pptx [S] |
| PNG/JPEG image | Yes | One slide, deck stacked vertically, or a long page up to 20,000 px | CDP or `capturePage` [S] |
| Project cover image | Yes | Render the first 1440×900 viewport; budget 5 s for resources, 8 s total | Hidden window [S] |
| HyperFrames MP4 | Yes, plus ffmpeg | Must run the GSAP timeline and seek frame by frame | CDP frame capture + ffmpeg (see below) [S] |
| Video cover image | No | Take the first frame | ffmpeg |
| Media generation (image, video, audio) | No | HTTP calls to the provider | fetch [S `media/`] |
| Live artifact render and refresh | No | Substitute template `{{data.x}}`, loop `data-od-repeat`; reject templates with script or iframe | Plain TS [S `live-artifacts/render.ts`] |
| Lint, deliverable check | No | Regex, AST, filesystem checks | [S] |
| PDF, DOCX, PPTX, XLSX preview | No | Text extraction: external `pdftotext`; DOCX, PPTX, XLSX unzip and read XML | [S `document-preview.ts`] |

**HyperFrames MP4 pipeline** [S `media/index.ts:3863–4200`, `media/hyperframes-runtime.ts`, `desktop/src/main/frame-capture.ts`]:
1. The agent creates a composition in `.hyperframes-cache/<id>`, with `hyperframes.json`, `meta.json` and `index.html`. The page must register `window.__timelines`.
2. Read width, height, fps (default 30) from the element with `data-composition-id`.
3. Inject the HyperFrames runtime (an external Apache-2.0 library) and the frame bridge.
4. For each frame i: seek to t = i/fps, capture a PNG.
5. ffmpeg: libx264, `yuv420p`, `+faststart`, pad dimensions to even numbers, 5-minute timeout.

**Boundary for the Rust version**: everything that needs a browser engine goes through **one `Renderer` trait** with 4 operations:
- `render_slides`: deck → list of PNGs;
- `render_frames`: timeline → PNGs at fps;
- `print_pdf`;
- `capture_page`.

The daemon does not need to know what sits underneath (see 5.3).

---

## 5. Proposed Rust architecture

### 5.1 Principles

- **Keep the daemon ↔ UI boundary as HTTP and SSE**, like the original app. Reason: the `od` CLI, the MCP server and the UI share one API, which preserves the dual-track rule; headless and Docker runs need no changes.
- **UI and daemon share one process on desktop**: axum binds a random loopback port and the webview points at it. No sidecar IPC, no HMAC.
- **Only one prompt set** (legacy). No rollout, no analytics.
- **No outbound network**, except calls to agents or providers the user configured. No telemetry, no silent auto-update.

### 5.2 Crate layout (Cargo workspace)

```
contour-design/
  crates/
    od-contracts/   serde DTOs: ChatRequest, AgentEvent (union from section 2.2), Project, ArtifactManifest…; emits JSON Schema for tests
    od-store/       SQLite (rusqlite, WAL, migrations by user_version) + directory layout + file versions + safe paths
    od-content/     Load and index skills, design templates, design systems, craft, prompt templates, plugins; parse frontmatter; assemble prompts
    od-runtime/     RuntimeDef (data), detection, spawn (process group), parser per stream format, ACP client, cancel/resume/steer, retry, loop guard
    od-runs/        Run registry: ring buffer, events.jsonl, state.json, SSE fan-out, replay
    od-preview/     Serve preview files, CSP, bridge injection, watcher (notify) → project events
    od-export/      standalone HTML, ZIP, transcript; Renderer trait + PDF/PPTX assembly (printpdf, a hand-written pptx crate on minimal OOXML)
    od-render-cdp/  Renderer via Chrome DevTools Protocol (chromiumoxide), using the Chrome/Edge already on the machine; ffmpeg for MP4
    od-media/       (P2) media provider adapters
    od-server/      axum: /api/* routes, origin/token middleware, SSE
    od-mcp/         MCP stdio server (rmcp) proxying to HTTP; od mcp install
    od-cli/         `od` binary (clap): daemon + client subcommands
    od-ui/          Frontend (Leptos CSR → WASM), built to static files, served by the daemon
  apps/
    desktop/        Tauri 2 shell: window, native dialogs, open links, embeds od-server in-process
  bridges/          JS bridges injected into the preview (deck, selection, snapshot, guard) — written fresh, not copied
  content/          skills/, design-templates/, design-systems/, craft/ … copied from the original app per section 3.7, with NOTICE
```

### 5.3 Choosing the UI

Root constraint: **the preview must be a real web engine**, because artifacts are agent-written HTML, CSS, JS. The only real choice is whether the app's own UI also runs in the webview.

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **Tauri 2 + Leptos (CSR/WASM)** | The preview is an iframe inside the same webview; the `postMessage` bridge keeps the same model. The UI can be opened in a normal browser when headless. Leptos has fine-grained reactivity, so chat streaming is fast. Tauri handles dialogs, menus, bundling | WASM adds about 1–2 MB to the bundle. Each OS has its own webview engine (WKWebView, WebView2, WebKitGTK), so the preview differs slightly from Chrome | **Chosen** |
| Tauri 2 + Dioxus (web) | Similar, React-like API, easy for React users | Smaller component ecosystem than Leptos for pure-web SPAs; Dioxus native desktop is still a webview | Viable alternative, on par with Leptos |
| GPUI + wry for preview | Very fast native UI (Zed-style) | GPUI is not stable outside Zed, sparse docs. Embedding a child webview in GPUI must be built by hand. Loses the browser UI mode | Not chosen |
| egui + wry | Simple, light | Text, i18n, RTL and rich Markdown are hard. Child webview position must be synced by hand, z-order bugs. Loses the browser UI | Not chosen |

**Export needs Chrome-identical pixels**: do not rely on the OS webview. Use `od-render-cdp` (chromiumoxide) to drive the Chrome, Edge or Chromium already on the machine, headless.
- Without Chrome: PDF falls back to webview printing (WKWebView has `createPDF`, WebView2 has `PrintToPdf`); slide capture reports "Chrome required".
- This is [Inf], and needs the owner's decision on whether downloading Chromium is allowed (section 5.6).

### 5.4 Runtime and storage

- Multi-threaded **tokio**. Each run has:
  - 1 task reading stdout by line (`tokio::io::BufReader::lines`, with a line-length cap);
  - 1 task reading stderr;
  - 1 watchdog task for timeouts.
- **Spawn**: `tokio::process::Command`. POSIX uses `process_group(0)` then `killpg`; Windows uses a Job Object to kill the whole tree.
- **Event bus**: `tokio::sync::broadcast` for live events, plus a `VecDeque` of 2,000 events as the ring. `events.jsonl` is written through a buffered writer, flushed every 50 ms or on a lifecycle event. `state.json` is written synchronously, via temp file then rename.
- **SQLite**: rusqlite, WAL, `synchronous=NORMAL`. One dedicated writer thread takes commands over a channel; multiple read-only connections. Message events are written in batches (like `message_event_batches`) and merged when the run ends.
- **Content index**: scanned once at startup (in parallel with rayon), held in an `ArcSwap`. User roots are watched with `notify` for refresh; no rescanning on every request as the original app does.
- **File serving**: tower-http `ServeFile` with ETag and Range. Bridge injection via `lol_html` (streaming rewriter), without parsing the whole DOM.
- **Watcher**: `notify` + `notify-debouncer-full`, 200 ms window; one watcher per project, reference-counted.

### 5.5 Performance strategy (measurable targets)

| Metric | Target | How measured |
|---|---|---|
| Cold start to usable UI | ≤ 800 ms on M-series | From process creation to `GET /api/health` plus first paint |
| Index all bundled content (about 450 skills, templates, systems) | ≤ 300 ms | `od-content` bench |
| Latency from an agent stdout line to the SSE event at the client | p99 ≤ 20 ms | Mock CLI emitting timestamps |
| File write to `file-changed` event | ≤ 300 ms | Integration test |
| Idle daemon RAM | ≤ 80 MB (excluding webview) | |
| Replay 50,000 events from jsonl | ≤ 500 ms | Bench |

### 5.6 Questions for the owner to decide (affect the design)

1. **Agent permissions**: the original app runs Claude with `bypassPermissions` and many CLIs with auto-approve flags (`--yolo`, `--force`, `--dangerously-…`). Does the Rust version keep that default, or add an "ask first" mode?
2. **Chrome for export**: use only an installed Chrome, or allow downloading Chromium (about 150 MB) when needed?
3. **Design systems named after real brands**: ship as-is, rename, or ship only a neutral set?
4. **Prompt set**: follow legacy (proposed), or the "Next" version as markdown plugins?

### 5.7 Six-milestone plan for the builder

Each milestone has done-criteria checkable with `cargo test` or e2e tests. Mock CLI: rewritten from the idea behind the original app's `mocks/`. It is a script that replays a pre-recorded JSONL file, placed first on PATH.

**M1: Data and content foundation** (about 1–1.5 weeks)
- Scope: `od-contracts`, `od-store`, `od-content`, `od-server` (health, projects, files, skills, design-templates, design-systems, craft), CLI `od project`, `od files`.
- Done when:
  - All of `content/` loads without errors. Item counts match directory counts (test compares against `ls`). Every `SKILL.md` parses to a `mode` among the 8 values.
  - Project CRUD round-trips over HTTP. `projects/<id>/` and `.open-design/project.json` are created correctly.
  - Path traversal (`../`, symlinks pointing outside, hidden segments in project import) all return 400 or 403 (test table ≥ 10 cases).
  - `GET /raw/<path>` has an ETag; resending with `If-None-Match` returns 304.
  - Create a version, list, restore: content returns byte-for-byte.

**M2: Run engine with Claude Code and Codex** (about 2 weeks)
- Scope: `od-runtime`, `od-runs`, `POST /api/runs`, SSE events, cancel, resume, legacy prompt assembly, `od run`.
- Done when:
  - Golden test: replaying the Claude stream recordings (original app fixtures) through the parser yields the agreed sequence of standard events. Each turn has exactly 1 `turn_end`; sub-agent frames do not trigger it.
  - Mock Claude runs two turns: turn 2 has `--resume <turn 1 id>`. Changing the model means no resume.
  - Drop SSE at event k, reconnect with `Last-Event-ID`: receive everything from k+1, no duplicates. Restart the daemon mid-run: the run gets `end{failed}`.
  - Cancel: the whole mock process tree (with spawned children) dies in ≤ 3.5 s; `end{canceled}`.
  - Steer after stdin is closed returns 409 `stdin_closed`.
  - Codex exec-json: a `command_execution` fixture yields a `tool_use` and `tool_result` pair named Bash.
  - Resending `POST /api/runs` with the same `clientRequestId` returns `reused: true`.

**M3: MVP UI and live preview** (about 2 weeks)
- Scope: `od-ui` (Leptos), Tauri shell, `od-preview`. Includes:
  - Home (brief box, prototype and deck chips, design system and template pickers);
  - project list;
  - Studio (chat stream, stop, question-form, file tree);
  - URL-load iframe preview plus watcher;
  - agent-picking onboarding;
  - agent and model Settings.
- Done when (e2e via WebDriver/tauri-driver, or Playwright against the headless build):
  - Golden scenario: enter a brief → mock agent writes `index.html` → the preview shows that content. Agent overwrites the file → preview changes within ≤ 1 s. Type turn 2 → the agent receives a prompt with the transcript or a resume.
  - A question-form in the stream renders as a form; submitting creates a user message with the formatted answers.
  - No network request leaves loopback during the whole scenario (checked with a blocking proxy).

**M4: Deck, bridges and editing on the preview** (about 1.5 weeks)
- Scope: srcDoc mode plus freshly written bridges (deck, selection/comment, snapshot, guards), deck viewer (keys, thumbnails, present), comments sent with the next turn, version history in the UI.
- Done when:
  - With a sample deck from `design-templates/`: next, prev, go(i) commands make the reported slide state return the right index; Home and End go to the ends.
  - Clicking an element in comment mode creates a comment with a stable selector. After a preview reload the comment still sticks to the right element.
  - A page with a `location.reload()` loop does not hang the host (redirect guard).

**M5: Export, CLI and MCP** (about 2 weeks)
- Scope: ZIP, standalone HTML, transcript, `od-render-cdp` (raster and vector PDF, PNG, image PPTX), `od export`, `od mcp` (20 tools, resources, active project), `od mcp install` (claude, codex, cursor first).
- Done when:
  - Standalone HTML opens with **networking off**, with no requests to any relative path (test re-reads it: no local relative URLs remain). Exceeding the limits returns 413.
  - 5-slide deck → 5-page PDF and 5-slide PPTX (re-read with a parser, count pages and slides). PNG is exactly 1920×1080.
  - MCP: a test client over stdio calls `list_projects`, `write_file`, `get_file`. Empty `project` uses the active project; after 5 minutes it reports a clear error.
  - `od mcp install cursor --print` prints valid JSON. A real install merges into the config without removing other entries (tested with a fake config file).

**M6: Extensions (P1 and P2)** (about 2–3 weeks, can be split)
- Scope: shared ACP client (Kimi, Hermes…), OpenCode, BYOK, bundled plugins as presets, media gen (OpenAI, fal, ElevenLabs), HyperFrames MP4, live artifact, i18n (en, vi).
- Done when:
  - ACP: a mock ACP agent goes through `initialize` → `session/new` → `session/prompt`. Permission requests are auto-approved per config. Cancel sends `session/cancel` before killing.
  - HyperFrames: a 2 s, 30 fps sample composition → MP4 with 60 frames (counted with ffprobe), h264-encoded with `yuv420p`.
  - Live artifact: a template with `<script>` is rejected. Refresh updates `data.json` and writes one line to `refreshes.jsonl`.
