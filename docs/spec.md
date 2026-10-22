# Spec hành vi: viết lại app gốc thành app local thuần Rust

- Nguồn phân tích: repo `nexu-io/open-design` (gọi tắt **app gốc**), commit `e03d5c2`, license Apache-2.0. Đọc source trực tiếp, không dịch ngược.
- Repo đích: `~/Documents/GitHub/open-design-rs` (chưa có code).
- Phạm vi: chỉ app local, một người dùng. **Bỏ hết** login, tài khoản, cloud, billing, team, share, collab, publish/deploy, telemetry.
- Ký hiệu nguồn của mỗi khẳng định:
  - **[S]**: đọc source, kèm đường dẫn (tương đối với gốc repo app gốc).
  - **[D]**: tài liệu trong repo (`docs/`, `AGENTS.md`, README).
  - **[Suy]**: suy ra từ phân tích, chưa kiểm bằng chạy thử.
- Không chạy app gốc. Mọi mô tả hành vi là đọc code. Spec chỉ mô tả, không dán code hay chuỗi UI nguyên văn.
- Quy mô app gốc để định cỡ: daemon khoảng 306k dòng TS, web khoảng 455k dòng TS/TSX [S]. Riêng `server.ts` khoảng 18,5k dòng, `FileViewer.tsx` khoảng 20k dòng [S]. Người viết lại **nên bám hành vi trong spec**, không bám cấu trúc file.

---

## 1. Bản đồ tính năng

Mức ưu tiên:
- **P0**: MVP chạy được: brief → agent sinh prototype HTML → live preview → sửa tiếp → export.
- **P1**: làm ngay sau MVP, đáng giá cao.
- **P2**: làm sau.
- **BỎ**: login, cloud, billing, team, share, telemetry, AMR/Vela, gói cloud trả phí.

### 1.1 Lõi project và chat

| Tính năng | Người dùng thấy gì | Source app gốc | Mức |
|---|---|---|---|
| Home: ô brief | Ô nhập lớn, Enter thì tạo project rồi vào project đó, gửi brief làm lượt đầu | `apps/web/src/components/HomeView.tsx`, `HomeHero.tsx`, `components/composer/*` [S] | P0 |
| Chip rail (loại đầu ra) | Hàng chip dưới ô nhập: prototype, deck, document, image, video, HyperFrames, audio, live-artifact… Chip chọn surface, skill mặc định, kind của project | `components/home-hero/chips.ts`, `runtime/home-intent.ts` [S]; glossary `CONTEXT.md` [D] | P0 cho prototype và deck, P2 cho phần còn lại |
| Chọn design system lúc tạo | Picker trong composer. Đổi system thì lần render sau dùng token mới | `components/DesignSystemPicker.tsx` [S] | P0 |
| Chọn template khởi đầu | Picker template; template được chọn **thay** skill mặc định làm `skillId` chính | `components/home-hero/TemplatePicker.tsx`; `docs/modes.md` [D] | P0 |
| Danh sách project | Lưới project có ảnh bìa, chấm trạng thái run. Tạo, đổi tên, nhân bản, xoá (có xác nhận) | `components/DesignsTab.tsx`, `components/project-actions/*` [S] | P0 (ảnh bìa P1) |
| Mở folder có sẵn làm project | Chọn thư mục bằng dialog native; project trỏ thẳng vào thư mục đó, không copy | `POST /api/import/folder`, `apps/daemon/src/import-export-routes.ts` [S] | P1 |
| Màn project (Studio) | Chia đôi: chat bên trái, workspace file và preview bên phải | `components/ProjectView.tsx` [S] | P0 |
| Chat stream | Lượt assistant chạy dần: text, khối thinking, dòng tool, tóm tắt file đã ghi, Markdown có tô màu code | `components/ChatPane.tsx`, `AssistantMessage.tsx`, `ToolCard.tsx` [S] | P0 |
| Stop | Nút dừng huỷ run đang chạy | `POST /api/runs/:id/cancel` [S] | P0 |
| Sửa tiếp (lượt sau) | Gõ tiếp trong cùng conversation; agent resume session nếu được | `apps/daemon/src/agent-session-resume.ts` [S] | P0 |
| Execution record và TodoWrite | Checklist các bước trong lượt. Pill "bước N/M" nổi trên composer khi run còn chạy, N là bước đang làm | `components/chat/ExecutionShell.tsx`, `runtime/chat/plan-pill.ts` [S]; `AGENTS.md` mục Chat UI [D] | P1 |
| Question form | Agent chèn khối `<question-form>` (JSON câu hỏi chọn hoặc nhập). UI vẽ thành form; nộp thì câu trả lời thành tin nhắn user kế tiếp | `apps/web/src/artifacts/question-form.ts`, `components/QuestionForm.tsx` [S]; `AGENTS.md` mục Asking the user questions [D] | P0 |
| Queue và steer | Gửi khi run đang chạy thì vào hàng đợi; sửa, xoá, kéo thả. Nút steer đẩy tin vào giữa lượt (chỉ Claude) | `components/chat/QueuedSendStack.tsx`, `apps/daemon/src/runtimes/run-steering.ts` [S] | P1 |
| Nhiều conversation, fork | Danh sách conversation trong project; fork từ một tin | `routes/project/conversations.ts` [S] | P1 |
| Session mode | Chọn design, chat hoặc plan; đổi phần đầu system prompt | `components/SessionModeToggle.tsx` [S] | P2 |
| Side chat tab | Tab chat phụ cùng agent | `components/workspace/SideChatTab.tsx` [S] | P2 |

### 1.2 Workspace, preview, sửa

| Tính năng | Người dùng thấy gì | Source app gốc | Mức |
|---|---|---|---|
| Cây file | Panel file có breadcrumb, upload, copy đường dẫn, nhóm theo artifact | `components/DesignFilesPanel.tsx`, `components/design-files/*` [S] | P0 |
| Quick switcher | Cmd/Ctrl+P, tìm mờ, file mở gần đây lên đầu | `components/QuickSwitcher.tsx` [S] | P1 |
| Tab workspace | Tab file, terminal, browser, sketch. Cmd/Ctrl+T, Cmd/Ctrl+W, Ctrl+Tab, Cmd/Ctrl+1–9 | `components/FileWorkspace.tsx`, `WorkspaceTabsBar.tsx` [S] | P1 (chỉ tab file), P2 (loại tab khác) |
| HTML preview | Iframe sandbox. Hai cách nạp: URL hoặc srcDoc (mục 4.1). Khung thiết bị desktop, tablet, phone | `components/file-viewer-render-mode.ts`, `runtime/srcdoc.ts` [S] | P0 |
| Live reload | File đổi trên đĩa thì preview tự nạp lại | `apps/daemon/src/project-watchers.ts`, `providers/project-events.ts` [S] | P0 |
| Viewer khác | Ảnh, SVG, video, audio, Markdown, text/code, PDF/DOCX/PPTX/XLSX (trích text), file nhị phân | `FileViewer.tsx` (các viewer con), `apps/daemon/src/document-preview.ts` [S] | P0 cho ảnh, SVG, text, Markdown; P1 cho phần còn lại |
| Deck viewer | Rail thumbnail, prev/next/first/last bằng phím, present mode (Esc thoát), speaker notes | `components/DeckThumbnailRail.tsx`, `runtime/slide-nav.ts`, `runtime/speaker-notes.ts` [S] | P0 cho điều hướng, P1 cho thumbnail, present và notes |
| Comment trên preview | Bật comment mode, rê hoặc click phần tử, ghim ghi chú. Ghi chú gửi kèm lượt sau làm ngữ cảnh | `runtime/comment-bridge.ts`, `routes/project/comments.ts` [S] | P1 |
| Inspect, chỉnh style | Chọn phần tử, chỉnh style trực tiếp (đặt, reset, trích) | `runtime/srcdoc.ts` (`od:inspect-*`) [S] | P2 |
| Sửa text trực tiếp | Sửa chữ ngay trong preview, ghi ngược vào source | `edit-mode/bridge.ts`, `edit-mode/source-patches.ts` [S] | P2 |
| Tweaks panel | Artifact tự khai báo panel tham số; host hiện panel đó | `design-templates/tweaks/`, `srcdoc.ts` (`od:tweaks-*`) [S] | P2 |
| Palette swap | Đổi nhanh bảng màu preview theo 5 preset | `srcdoc.ts` (palette bridge) [S] | P2 |
| Vẽ chú thích | Vẽ đè lên preview rồi gửi ảnh chụp cho agent | `components/PreviewDrawOverlay.tsx` [S] | P2 |
| Lịch sử phiên bản file | Danh sách version theo nguồn (AI, tay, restore); xem, khôi phục | `apps/daemon/src/project-file-versions.ts`, `FileViewer.tsx` [S] | P1 |
| Lint artifact | Lint bằng regex các mẫu "AI slop" (gradient tím, emoji làm icon, số liệu bịa…), trả kết quả cho agent | `apps/daemon/src/lint-artifact.ts` [S] | P2 |
| Kiểm deliverable sau run | Kiểm run có thật sự tạo hoặc chạm file đầu ra đúng loại không | `apps/daemon/src/run-deliverable-validation.ts` [S] | P1 |

### 1.3 Export

| Tính năng | Hành vi | Source app gốc | Mức |
|---|---|---|---|
| ZIP | Tải cả project hoặc thư mục con thành zip | `GET /api/projects/:id/archive` [S] | P0 |
| HTML standalone | Gói một file HTML tự chứa: CSS, ảnh, font, module đều nhúng thành `data:` | `apps/daemon/src/artifacts/standalone-html.ts` [S] | P0 |
| PDF | In vector, hoặc chụp raster từng slide/trang rồi ghép | `apps/desktop/src/main/pdf-export.ts`, `deck-export.ts` [S] | P1 |
| Ảnh PNG/JPEG | Chụp một slide, cả deck ghép dọc, hoặc cả trang | `apps/desktop/src/main/artifact-export.ts` [S] | P1 |
| PPTX (ảnh) | Mỗi slide một ảnh tràn khung | `apps/daemon/src/deck-export.ts` [S] | P1 |
| PPTX (editable) | Chuyển DOM slide thành shape và text native | `apps/desktop/vendor/dom-to-pptx` [S] | P2 |
| MP4 HyperFrames | Seek timeline từng frame, chụp, ghép bằng ffmpeg | `apps/daemon/src/media/index.ts`, `apps/desktop/src/main/frame-capture.ts` [S] | P2 |
| Handoff | Mở folder trong editor, copy prompt cho coding agent, sinh `DESIGN-HANDOFF.md` | `components/HandoffButton.tsx`, `FinalizeDesignButton.tsx` [S] | P1 |

### 1.4 Nội dung và mở rộng

| Tính năng | Hành vi | Source app gốc | Mức |
|---|---|---|---|
| Thư viện skill | Liệt kê, xem, import, xoá skill của user; user skill đè skill bundled cùng id | `apps/daemon/src/skills.ts`; `docs/skills-protocol.md` [S][D] | P0 cho đọc, P1 cho CRUD |
| Design template | Catalogue template render được (prototype, deck…) | `/api/design-templates` [S] | P0 |
| Design system | Duyệt khoảng 150 package; xem kit (màu, chữ, logo, giọng văn); đặt mặc định | `components/DesignSystemsTab.tsx`, `apps/daemon/src/design-systems/` [S] | P0 cho đọc và chọn, P1 cho trang duyệt |
| Tạo design system | Từ ảnh chụp, URL, repo GitHub hoặc thư mục; agent viết `DESIGN.md` | `POST /api/design-systems/import/*`, `generation-jobs` [S] | P2 |
| Craft rules | Luật thiết kế chung, chèn vào prompt khi skill hoặc system yêu cầu | `apps/daemon/src/craft.ts`, `craft/` [S] | P0 |
| Plugin và marketplace | Trang plugin, "Try it", form input, cài từ registry, trust/capability | `routes/plugins/*`, `docs/plugins-spec.md` [S][D] | P2 (chỉ đọc plugin bundled làm preset: P1) |
| Atoms và pipeline | Các bước pipeline (critique, todo, file…) khai báo trong plugin | `apps/daemon/src/plugins/atoms.ts`, `pipeline-runner.ts` [S] | P2 |
| Prompt template media | Thư viện prompt ảnh và video (JSON) | `prompt-templates/`, `media/prompt-templates.ts` [S] | P2 |

### 1.5 Agent, cài đặt, tích hợp

| Tính năng | Hành vi | Source app gốc | Mức |
|---|---|---|---|
| Phát hiện agent | Quét các CLI đã cài, đo version, model, trạng thái auth | `apps/daemon/src/runtimes/detection.ts` [S] | P0 |
| Runtime Claude Code | Spawn `claude` stream-json, parse stream, resume, steer | `runtimes/defs/claude.ts`, `claude-stream.ts` [S] | P0 |
| Runtime Codex | Spawn `codex` (exec-json hoặc app-server) | `runtimes/defs/codex.ts` [S] | P0 cho exec-json, P1 cho app-server |
| Runtime khác (OpenCode, Cursor, Copilot, Qoder, Pi, dsh, plain text…) | Mỗi CLI một bộ argv và parser | `runtimes/defs/*.ts` [S] | P1 cho OpenCode và Cursor; P2 cho phần còn lại |
| Họ ACP (Devin, Hermes, Kimi, Kiro, Kilo, Vibe, Trae, Reasonix) | JSON-RPC qua stdio theo Agent Client Protocol | `agent-protocol/acp/session.ts`; `docs/new-agent-runtime-acp.md` [S][D] | P1 (một client ACP chung phủ 8 agent) |
| BYOK (API key) | Không có CLI: dùng key của user; gọi qua OpenCode hoặc proxy stream trực tiếp | `runtimes/byok-opencode.ts`, `routes/chat.ts` (`/api/proxy/*`) [S] | P1 |
| Onboarding | Lần đầu: chọn agent local hoặc API key, test kết nối, cho phép bỏ qua | `EntryShell.tsx` (`OnboardingView`) [S] | P0 (bỏ bước cloud) |
| Settings | Chọn agent, model, reasoning; key BYOK; MCP; media provider; ngôn ngữ; custom instructions | `components/SettingsDialog.tsx` [S] | P0 cho agent và model; P1 cho phần còn lại |
| MCP server cho agent ngoài | `od mcp` qua stdio; agent khác đọc và ghi file project | `apps/daemon/src/mcp.ts` [S] | P1 |
| `od mcp install <agent>` | Ghi config MCP vào 17 agent khác | `apps/daemon/src/mcp-agent-install.ts` [S] | P1 |
| MCP client | Daemon nối tới MCP server của user (stdio, SSE, HTTP, OAuth 2.1) rồi truyền cho agent lúc spawn | `mcp-config.ts`, `mcp-oauth.ts` [S] | P2 |
| CLI `od` | Mọi tính năng có subcommand gọi cùng HTTP API | `apps/daemon/src/cli.ts` [S]; `AGENTS.md` dual-track [D] | P0 cho project, files, run, export; P1 cho phần còn lại |
| Media gen (ảnh, video, audio) | Gọi provider bằng key của user, ghi file vào project | `apps/daemon/src/media/` [S] | P2 |
| Live artifact | Trang HTML từ template cộng data, refresh tay từ nguồn | `apps/daemon/src/live-artifacts/` [S] | P2 |
| Automations, routines | Hẹn giờ chạy prompt định kỳ | `routines.ts`, `components/TasksView.tsx` [S] | P2 |
| Memory | Lưu sở thích và luật của user qua các session | `memory*.ts`, `components/MemorySection.tsx` [S] | P2 |
| Connectors (Composio) | Nối dịch vụ ngoài qua Composio | `apps/daemon/src/connectors/` [S] | P2 (phụ thuộc dịch vụ bên thứ ba, cân nhắc BỎ) |
| Critique theater | Hội đồng nhiều vai chấm điểm thiết kế theo vòng | `components/Theater/*`, `apps/daemon/src/critique/` [S]; `docs/critique-theater.md` [D] | P2 |
| Terminal | Tab terminal nối PTY, reconnect bằng Last-Event-ID | `apps/daemon/src/terminals.ts`, `TerminalViewer.tsx` [S] | P2 |
| Figma import, sketch, library, clipper, pets, what's new | Tiện ích phụ | `FigmaImportModal.tsx`, `SketchEditor.tsx`, `LibrarySection.tsx`, `components/pet/*` [S] | P2 (pets, what's new: cân nhắc BỎ) |
| i18n | 19 locale, có RTL | `apps/web/src/i18n/` [S] | P1 (en và vi trước) |
| Theme | Chỉ theme sáng, chọn màu accent | `state/appearance.ts` [S] | P1 |
| Desktop shell | Cửa sổ chính, menu native, dialog chọn folder, mở link ngoài | `apps/desktop/src/main/*` [S] | P0 |
| Auto-update | Kênh update, kiểm checksum | `apps/desktop/src/main/updater*` [S] | P2 |

### 1.6 BỎ hẳn (không port)

| Nhóm | Source app gốc |
|---|---|
| Đăng nhập, tài khoản, AMR Cloud, Vela, ví, billing, gói trả phí, campaign | Daemon: `routes/vela.ts`, `integrations/vela*.ts`, `runtimes/defs/amr.ts`, `runtimes/amr-*`, `media/vela*.ts`, `storage/amr-*`. Web: `components/Amr*`, `CloudSignInTip.tsx`, `CodingPlanUsage.tsx`, `PlanBadge.tsx`, `GoPlanSunsetDialog.tsx`, `campaigns/*`, `runtime/amr-*.ts`, MCP tool `start_vela_login`, CLI `od amr` [S] |
| Team, share, collab, invite, workspace, publish public | Daemon: `collab/` (109 file, khoảng 25k dòng), `routes/collab-*.ts`, `routes/team-*.ts`, `routes/social-share.ts`, `design-systems/{team-*,workspace-*}.ts`. Web: `collab/*`, `components/share/*`, `InviteDialog.tsx`, các route team. Desktop: `invite-deeplink*.ts`. CLI `od collab`, `od workspace`, `od share` [S] |
| Deploy (Cloudflare Pages, Vercel) | `deploy.ts`, `deploy/`, `routes/deploy.ts`, bảng `deployments` [S] |
| Telemetry, analytics, Langfuse, OTLP, khảo sát, upload chẩn đoán | Daemon: `analytics.ts`, `langfuse-*.ts`, `observability/`, `routes/telemetry.ts`, `storage/telemetry-outbox.ts`, `diagnostics-*.ts`, `services/*diagnostic*`. Web: `analytics/*`, `observability/*`, `PrivacyConsentModal.tsx`, `ExperienceSurvey.tsx`. Desktop: `observability.ts`, `crash-diagnostics.ts` [S] |
| Badge GitHub stars, Discord presence, metadata công khai | `GithubStarBadge.tsx`, `useDiscordPresence.ts`, `routes/open-design-public-metadata.ts` [S] |
| Kênh phát hành beta, prerelease, preview, stable; Feishu; CI release | `tools/release`, `tools/pack` (phần channel) [D] |
| Strategy rollout "OD Next" và A/B prompt | `strategies/od-next/rollout*.ts` [S]. Rust chỉ giữ **một** bộ prompt (xem mục 2.5) |

---

## 2. Kiến trúc hiện tại

### 2.1 Tiến trình và vai trò

```
Electron shell ──(sidecar IPC)── daemon (Express, Node 24) ── spawn CLI agent (cwd = thư mục project)
     │                                   │
     └─ renderer: web Next.js ─HTTP+SSE──┘  SQLite + file trên đĩa
```

- **Daemon** là nguồn sự thật duy nhất cho `/api/*`, khoảng 400 route [S `apps/daemon/src/server.ts`, `routes/**`]. Daemon lo:
  - lưu project, conversation, message;
  - file, version, preview, import, export;
  - phát hiện, spawn, stream, huỷ agent;
  - ghép prompt;
  - plugin, MCP, media, memory.
- **Web** là SPA. Next.js chỉ rewrite `/api/*`, `/artifacts/*`, `/frames/*` sang daemon. Ở bản đóng gói, daemon tự serve bản export tĩnh `apps/web/out` [D `docs/architecture.md` §1–2].
- **Electron** lo: cửa sổ, dialog native, giao thức `od://`, auto-update, và mọi việc cần browser engine để export (PDF, chụp slide, chụp frame). Daemon gọi Electron qua IPC với 4 action: `export-pdf`, `export-artifact`, `render-slides`, `render-frames` [S `packages/sidecar-proto/src/index.ts:104–110`].
- **CLI `od`** cũng chính là binary của daemon: `od` không kèm subcommand thì khởi động daemon. Port mặc định 7456, host `127.0.0.1` [S `cli.ts:9885`, `daemon-startup.ts`].

### 2.2 Giao thức web ↔ daemon

- **Chỉ HTTP và SSE, không WebSocket**: không có dependency `ws`, không có upgrade handler [S].
- **Định dạng SSE**: mỗi event là `id: N` / `event: <tên>` / `data: <json>`. Header `no-cache, no-transform` và `X-Accel-Buffering: no`. Gửi comment keepalive mỗi 25 s [S `server.ts:3082–3095`].
- **Tên event SSE của run**: `start`, `run_retry_attempted`, `agent`, `stdout`, `stderr`, `diagnostic`, `error`, `end` [S `packages/contracts/src/sse/chat.ts:510`].
  - `start` mang runId, agentId, model, `protocolVersion: 1`.
  - `end` mang `status` (succeeded, failed, canceled), mã thoát, danh sách file artifact, lỗi đã phân loại, cờ `resumable` và `retryable`.
- **Payload của `agent`** (trường `type`), là union chuẩn hoá cho mọi CLI [S `sse/chat.ts:171–446`]:

  | Nhóm | Type |
  |---|---|
  | Trạng thái | `status` (label, model, sessionId, ttftMs) |
  | Text, thinking | `text_delta`, `thinking_start`, `thinking_delta`, `thinking_tokens` |
  | Tool | `tool_use` (id, name, input), `tool_result` (toolUseId, content, isError), `tool_input_delta`/`_target`/`_progress`, `tool_in_flight` (chỉ phát live, không lưu) |
  | Chi phí | `usage` (token, cost, stopReason), `request_usage` |
  | Daemon tự sinh | `conversation_title`, `next_steps`, `artifact_focus`, `artifact` (cho runtime plain), `live_artifact*` |
  | Bảo vệ | `tool_loop`, `fabricated_role_marker`, `raw` (dòng không parse được, có giới hạn độ dài) |

- **Replay và reconnect** [S `runtimes/runs.ts:727, 1485–2000`]:
  - Mỗi run có event id tăng dần. Có ring buffer 2.000 event trong RAM.
  - Mọi event đồng thời được append vào `runs/<runId>/events.jsonl`. `start`, `error`, `end` ghi đồng bộ vào `runs/<runId>/state.json`.
  - Client gửi `Last-Event-ID` hoặc `?after=N`. Phần cũ hơn ring đọc lại từ jsonl; event live đến trong lúc replay được xếp hàng và khử trùng theo id. Run đã kết thúc thì gửi hết lịch sử rồi đóng stream.
  - Daemon khởi động lại: run đã kết thúc nạp lại từ đĩa; run đang chạy dở thì được ghi thêm `error` cộng `end{failed}`.
  - **Tiêu chí cho bản Rust**: ngắt kết nối ở event k, nối lại với `Last-Event-ID: k`, phải nhận đúng từ k+1, không trùng, không thiếu.
- **Endpoint chính cho P0** [S `routes/project/index.ts`, `routes/runs.ts`, `import-export-routes.ts`]:

  | Nhóm | Endpoint |
  |---|---|
  | Sức khoẻ | `GET /api/health`, `/api/version` |
  | Agent | `GET /api/agents` (có chế độ stream kết quả phát hiện) |
  | Nội dung | `GET /api/skills`, `/api/design-templates`, `/api/design-systems[/:id]`, `/api/craft` |
  | Project | `GET/POST /api/projects`, `GET/PATCH/DELETE /api/projects/:id`, `POST .../duplicate` |
  | File | `GET/POST .../files`, `GET .../files/<path>`, `POST .../files/rename`, `DELETE .../files/<path>`, `POST .../upload` |
  | Preview | `GET .../raw/<path>` (Range, ETag), `GET .../preview-url`, `GET .../preview/<scope>/<path>` |
  | Version | `GET/POST .../files/<path>/versions`, `POST .../versions/:vid/restore` |
  | Conversation | `GET/POST .../conversations`, `GET .../conversations/:cid/messages`, `PUT .../messages/:mid` |
  | Run | `POST /api/runs` → 202 `{runId…}` (idempotent theo `clientRequestId`); `POST /api/chat` (tạo run và stream luôn trong response); `GET /api/runs/:id/events` (SSE); `POST /api/runs/:id/cancel`; `POST /api/runs/:id/steer` |
  | Sự kiện project | `GET /api/projects/:id/events` (SSE: `ready`, `file-changed {path, kind: add/change/unlink}`, `conversation-created`) |
  | Export | `POST .../export/{html,pdf,pdf-image,image,pptx}`, `GET .../archive` |
  | Cấu hình | `GET/PUT /api/app-config`, `GET/PUT /api/media/config` |
  | Active project | `POST/GET /api/active` |

- **Body của `POST /api/runs`** (`ChatRequest`, các trường chính) [S `packages/contracts/src/api/chat.ts:116`]: agentId, message, projectId, conversationId, các id message (client tự sinh), clientRequestId, skillId, skillIds, designSystemId, attachments, commentAttachments, model, reasoning, sessionMode, locale.
- **Bảo mật** [S `server.ts:3226–3557`, `api-token-auth.ts`, `origin-validation.ts`]:
  - Mặc định bind loopback, không cần auth.
  - Bind ra ngoài loopback thì bắt buộc có `OD_API_TOKEN` (Bearer hoặc Basic).
  - Origin guard: request không có `Origin` (CLI) thì cho qua. `Origin: null` chỉ được GET file preview. Origin khác phải là loopback hoặc LAN đúng port, hoặc nằm trong `OD_ALLOWED_ORIGINS`.
  - Agent gọi ngược daemon bằng `OD_TOOL_TOKEN`: TTL ít nhất 15 phút, chỉ dùng cho một run, chỉ cho phép danh sách endpoint tool [S `tool-tokens.ts`].
  - **Bản Rust nên giữ**: loopback mặc định, origin guard, tool token theo run. **Bỏ**: HMAC desktop cho folder import, vì shell Rust gọi thẳng trong process.

### 2.3 Launch và stream coding agent

**Mô hình chung** [S `runtimes/types.ts:145–338`, `runtimes/registry.ts`; D `docs/agent-adapters.md`]:
- Mỗi agent là **một bản khai báo dữ liệu**, không có code `run()` riêng. Bản khai báo gồm:
  - binary và các binary dự phòng;
  - hàm dựng argv từ prompt, ảnh, thư mục phụ, model, reasoning, session;
  - `streamFormat` và parser tương ứng;
  - cách đưa prompt vào: stdin, file hay argv; `text` hay `stream-json`;
  - cờ resume;
  - timeout;
  - probe version, help, model, auth;
  - cách tiêm cấu hình MCP.
- Một engine chung lo phát hiện, spawn, parse, huỷ.
- **Bản Rust nên giữ đúng mô hình này**: trait `RuntimeDef` thuần dữ liệu cộng một `RunEngine`.

**Phát hiện agent** [S `runtimes/detection.ts`]:
- Resolve đúng đường dẫn binary mà run sẽ spawn, rồi chạy probe version.
  - Không có binary hoặc không thực thi được: không khả dụng.
  - Chạy được nhưng từ chối cờ version: vẫn khả dụng.
- Sau đó chạy song song probe help (tìm chuỗi cờ trong `--help`), probe model, probe auth. Mỗi probe cô lập, một CLI hỏng không làm rỗng danh sách. Không cache ra đĩa.

**Spawn** [S `server.ts:14864–15093`, `runtimes/agent-process.ts`, `runtimes/env.ts`]:
- cwd là thư mục project.
- POSIX: agent là trưởng process group riêng (detached) để kill cả cây.
- Runtime nhận cả prompt một lần thì nhận prompt qua stdin trỏ vào file (all-or-nothing). Claude stream-json, ACP, pi, dsh và codex app-server giữ pipe mở.
- Ghi record tiến trình vào `runs/<runId>`. Lần khởi động sau, daemon dọn các agent mồ côi.
- **Env truyền vào agent**:
  - `OD_DATA_DIR`, `OD_DAEMON_URL`, `OD_TOOL_TOKEN`, `OD_PROJECT_ID`, `OD_PROJECT_DIR`, `OD_BIN` (đường dẫn `od` để agent gọi tool);
  - riêng Claude: `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`;
  - Không bao giờ truyền `OD_API_TOKEN`.
- **Skill được stage**: copy skill đang dùng vào `<cwd>/.od-skills/<tên>-<hash>/`, không dùng symlink. Thân SKILL.md có thêm đoạn đầu chỉ đường tới các file phụ [S `skills.ts` `withSkillRootPreamble`; D `docs/skills-protocol.md` §3].

**Claude Code** [S `runtimes/defs/claude.ts:67–117`, `run-steering.ts:95–103`, `claude-stream.ts`]:
- **argv**: `-p --input-format stream-json --output-format stream-json --verbose`, thêm có điều kiện:
  - `--include-partial-messages`, nếu `-p --help` có cờ này;
  - `--model <m>`, trừ khi model là `default`;
  - `--add-dir <dirs>`;
  - `--resume <id>` nếu resume, ngược lại `--session-id <uuid mới>`;
  - luôn có `--permission-mode bypassPermissions`. Người viết lại cần **quyết định có giữ không**: đây là lựa chọn an toàn, xem mục 5.6.
- **Prompt**: một dòng JSONL dạng user message (role user, content là một khối text), ghi vào stdin, **giữ stdin mở**.
- **MCP**: ghi `.mcp.json` vào cwd, chỉ khi cwd là thư mục project do daemon quản lý.
- **Steer**: ghi thêm một dòng user message cùng định dạng, chỉ khi run còn sống và stdin còn mở. Lỗi trả 409 với lý do `runtime_unsupported`, `run_terminal` hoặc `stdin_closed`.
- **Ranh giới lượt và đóng stdin**: đọc `stop_reason` theo thứ tự ưu tiên:
  1. `stream_event` → `message_delta.delta.stop_reason` (bản CLI mới, cần `--include-partial-messages`);
  2. `assistant.message.stop_reason` (bản cũ);
  3. frame `result` cuối lượt.
  - Khử trùng theo message id. Bỏ qua frame của sub-agent (có `parent_tool_use_id`).
  - `stop_reason` khác `tool_use` thì lượt kết thúc và đóng stdin. Gặp `tool_use` mà đóng stdin thì sẽ cắt cụt câu trả lời [D `AGENTS.md` Agent runtime conventions].
- **Ánh xạ frame Claude sang event chuẩn**: `system/init` → `status`; `content_block_delta` → `text_delta`/`thinking_delta`/`tool_input_delta`; `content_block_stop` → `tool_use`; khối `tool_result` trong frame `user` → `tool_result`; `result` → `usage`, kèm `error` nếu `is_error`. Lời gọi `TaskCreate`/`TaskUpdate` gộp thành một snapshot `TodoWrite` [S `claude-stream.ts:304–345, 717–1160`].
- **Fixture để test**: app gốc có bản ghi stream thật trong `apps/daemon/tests/fixtures/claude-cli-recordings/` [D]. Đây là dữ liệu Apache-2.0, dùng làm golden input được (cần NOTICE).

**Codex** [S `runtimes/defs/codex.ts:327–688`]:
- **exec-json** (nên làm trước):
  - `exec --json --skip-git-repo-check --sandbox workspace-write`;
  - các `-c`: bật network cho sandbox, `model_reasoning_summary`, bật `update_plan`;
  - `-C <cwd>`, `--add-dir`, `--model`;
  - prompt qua stdin.
  - Resume: `exec resume --json … <thread_id>`.
- **app-server** (mặc định ở app gốc): JSON-RPC qua stdio (`initialize`, `thread/start|resume`, `turn/start`, `turn/interrupt`) [S `agent-protocol/codex-app-server/session.ts`].
- **Ánh xạ** [S `json-event-stream.ts:1161–1441`]:

  | Frame Codex | Event chuẩn |
  |---|---|
  | `thread.started` | `status{sessionId}` |
  | `command_execution` | `tool_use` tên Bash + `tool_result` (lỗi nếu exit ≠ 0) |
  | `file_change` | `tool_use` Write/Edit theo từng file |
  | `todo_list` | `TodoWrite` |
  | `agent_message` | `text_delta` |
  | reasoning | `thinking_delta` |
  | `turn.completed.usage` | `usage` |

**Runtime khác, tóm tắt** [S `runtimes/defs/*.ts`]:

| Runtime | Lệnh | Prompt | Parser |
|---|---|---|---|
| Cursor Agent | `cursor-agent --print --output-format stream-json --stream-partial-output --force` | stdin | json-event-stream (cursor) |
| OpenCode | `opencode run --format json [-s session] [-m provider/model]`, config MCP qua env | stdin | json-event-stream (opencode) |
| Copilot | `copilot --allow-all-tools --output-format json` | stdin | riêng |
| Họ ACP | `<bin> acp` hoặc tương đương; JSON-RPC `initialize` → `session/new` hoặc `session/load` → `session/prompt`; tự duyệt `session/request_permission` | RPC | ACP chung |
| DeepSeek Harness (dsh) | `dsh --profile <tên> --stdio`; giao thức JSONL riêng (`execute`, `cancel` / `text`, `tool_call`, `usage`, `result`) | RPC | dsh-profile |
| Plain text (Qwen, Aider…) | stdout là text thô; cuối run tách các khối `<artifact>` thành file | argv, stdin hoặc file | plain |

- **Thứ tự port gợi ý**: Claude → Codex exec-json → ACP chung → OpenCode → phần còn lại.

**Huỷ** [S `runtimes/runs.ts:2228–2526`]:
- Nếu giao thức có lệnh abort (ACP `session/cancel`, dsh `cancel`) thì gửi trước, chờ 3 s.
- Sau đó đóng stdin, gửi SIGTERM cho **cả process group**, chờ 3 s, rồi SIGKILL group.
- Windows: chụp cây tiến trình rồi leo thang theo cùng thứ tự.
- Tín hiệu gắn với đúng child và pgid của lần thử đó, để lần retry không bị kill nhầm.

**Resume** [S `agent-session-resume.ts:121–436`]:
- Bảng `agent_sessions`, khoá (conversation, agent), lưu sessionId, model, cwd, message cuối, hash phần prompt ổn định.
- Không resume (gửi lại toàn bộ transcript) khi model đổi, cwd đổi, thiếu con trỏ, hoặc conversation đã đi tiếp.
- Hai kiểu lấy session id:
  - Claude: daemon tự tạo UUID, truyền qua `--session-id`;
  - Codex, OpenCode, ACP: bắt id từ stream.
- Resume thất bại (nhận qua stderr, hoặc qua `result` lỗi với 0 turn) thì xoá handle và tự chạy lại một lần, không báo lỗi cho user.

**Retry, timeout, guard** [S `run-retry-policy.ts`, `runtimes/chat-run-lifecycle.ts`, `tool-loop-guard.ts`]:
- **Retry**: tối đa 1 lần, chỉ khi lỗi tạm thời và **chưa có output nào hiển thị cho user**. Backoff 500 ms (1.000 ms nếu bị rate limit), nhân đôi tới 8 s, có jitter.
- **Timeout**: không có output trong 10 phút thì dừng (Copilot 30 phút).
- **Tool-loop guard**: 5 lỗi liên tiếp thì cảnh báo, 10 thì dừng. Cùng một hành động lỗi 4 lần thì cảnh báo, 8 thì dừng. Mặc định chỉ cảnh báo.

### 2.4 Ghép prompt

- Toàn bộ prompt đã ghép là **một user message**, không dùng cờ system prompt của CLI [S `runtimes/chat-prompt-inputs.ts:115–307`].
- **Hai khối**: phần hướng dẫn (đọc trước) rồi phần yêu cầu của user, kèm attachment và ảnh.
- **Thứ tự các khối trong phần hướng dẫn** (bản legacy) [S `apps/daemon/src/prompts/system.ts:1040–1528`]:
  1. chống prompt injection;
  2. override theo mode (plan, chat, API);
  3. locale;
  4. discovery: hướng dẫn hỏi lại bằng question-form;
  5. thư viện hướng visual, bỏ khi đã có design system;
  6. khung thiết bị và hợp đồng nền tảng;
  7. design system đang dùng: `DESIGN.md`, `tokens.css`;
  8. craft;
  9. skill đang dùng;
  10. plugin;
  11. deck framework;
  12. hợp đồng media;
  13. critique;
  14. định danh comment.
  - Khối thay đổi theo lượt để cuối, để phần đầu ổn định cho prompt cache.
- **Design system cũng có thứ tự riêng** [D `docs/skills-protocol.md` §5]: `USAGE.md` → `DESIGN.md` → hướng dẫn import mode → `tokens.css` → manifest component → mục lục file giàu nội dung (agent tự đọc khi cần) → craft → thân skill.
- **Hai implementation song song** (legacy và "OD Next"), chọn theo từng run [D `docs/prompt-composition.md`; `AGENTS.md` Prompt variants]. Bản Rust **chỉ làm một**. Đề xuất bám bản legacy vì cấu trúc rõ, nằm gọn trong `apps/daemon/src/prompts/`.
- **Hai profile thực thi** [D `docs/architecture.md` §4]:
  - Runtime có tool file (Claude, Codex…): agent tự ghi file vào project; preview đi theo file.
  - Runtime plain hoặc BYOK không có tool: model trả **một** khối `<artifact>` đầy đủ; host tách ra rồi ghi thành file.

### 2.5 MCP server `od mcp`

- **Transport**: chỉ stdio. Server không giữ state, mọi tool là một lời gọi HTTP tới daemon. Daemon tắt thì vẫn liệt kê được tool. Tự thoát sau 30 phút rảnh [S `apps/daemon/src/mcp.ts`, `mcp-bootstrap.ts`].
- **Tìm daemon**: `--daemon-url` → `OD_DAEMON_URL` → trạng thái sidecar → mặc định `http://127.0.0.1:7456`.
- **Tool** (22, bỏ 2 tool login thì còn 20) [S `mcp.ts:365–860`]:
  - đọc: `list_projects`, `get_project`, `get_active_context`, `get_artifact`, `get_file` (theo dòng, mặc định 2.000), `search_files` (literal, không phân biệt hoa thường, mặc định 200 kết quả), `list_files`;
  - ghi: `create_artifact` (từ chối nếu file đã có), `write_file`, `delete_file`, `delete_project` (bắt buộc id và `confirm: true`), `create_project`;
  - catalogue: `list_skills`, `list_plugins`, `list_agents`;
  - run: `start_run`, `get_run`, `cancel_run`;
  - brief: `collect_brief` và `confirm_brief`, mở form brief qua MCP Apps.
- **Resource**: `od://focus/active`, `od://skills/<id>/SKILL.md`, `od://design-systems/<id>/DESIGN.md`.
- **Active Project** [S `routes/active-context.ts`, `constants.ts`]:
  - Web POST `/api/active` mỗi lần đổi route. Chỉ giữ trong RAM, TTL 5 phút.
  - Tool MCP bỏ trống `project` thì dùng project đang active; response ghi rõ đã dùng active context.
- **Server MCP thứ hai** `od mcp live-artifacts`: tạo, liệt kê, cập nhật, refresh live artifact và connector [S `mcp-live-artifacts-server.ts`].
- **`od mcp install <agent>`**: 17 agent, 3 cách cài [S `mcp-agent-install.ts`]:

  | Cách | Agent |
  |---|---|
  | Gọi lệnh `mcp add` của chính CLI đó | claude, codex, kimi, reasonix |
  | Deep-merge một entry vào file JSON config | cursor, copilot, cline, opencode, openclaw, antigravity, kiro, trae, raven, claude-desktop |
  | Chỉ in snippet | pi, hermes, vibe |

  Có `--print` (chạy thử) và `--uninstall`.

### 2.6 CLI `od`

- Dispatch qua `SUBCOMMAND_MAP`. Hầu hết subcommand là client mỏng gọi `/api/*`, có `--json` và `--daemon-url` [S `cli.ts:404`; D `AGENTS.md` dual-track].
- **Nhóm nên giữ**:

  | Nhóm | Subcommand |
  |---|---|
  | Project | `project` (create, list, info, delete, duplicate, import-folder), `conversation` |
  | Run | `run` (start, watch dạng NDJSON, cancel, steer, continue, list, info) |
  | File | `files` (list, read, write, upload, delete, diff, versions…) |
  | Export, kiểm tra | `export`, `lint` |
  | Nội dung | `skills`, `design-systems`, `craft`, `templates`, `plugin` (list, info, apply, validate) |
  | Media | `media` (generate, scaffold, wait) |
  | MCP | `mcp`, `mcp install` |
  | Daemon | `daemon` (start, status, stop), `status`, `version`, `doctor`, `config` |
  | Agent tự gọi | `od tools …` (live-artifacts, design-systems read, deliverable-syntax), dùng tool token |

- **Nhóm BỎ**: `amr`, `collab`, `workspace`, `share`, `deploy`, `strategy rollout`, `diagnostics export`, và các lệnh `plugin` login, publish, yank.
- **Mã thoát cần giữ**: 66 khi plugin thiếu capability [D `docs/plugins-spec.md` §9.1]; 1 khi `od lint` có lỗi đạt ngưỡng `--fail-on`.

---

## 3. Định dạng dữ liệu

### 3.1 Thư mục dữ liệu

- **Gốc dữ liệu** = `OD_DATA_DIR`, resolve một lần lúc khởi động. Mọi đường dẫn dữ liệu đều suy ra từ gốc này [D `AGENTS.md` Daemon data directory contract].
- **Bên dưới gốc** [S `server.ts:1420–1491`]:

  | Đường dẫn | Nội dung |
  |---|---|
  | `app.sqlite` | Database (chế độ WAL) |
  | `app-config.json`, `media-config.json` | Cấu hình app, cấu hình media |
  | `projects/<projectId>/` | Project do daemon quản lý |
  | `runs/<runId>/state.json`, `events.jsonl` | Trạng thái và log event của run |
  | `artifacts/<stamp>-<slug>/index.html` | Artifact lưu rời |
  | `chat-artifact-blobs/objects/aa/bb/<sha256>` | Kho nội dung theo hash cho snapshot thẻ chat |
  | `skills/`, `design-templates/`, `design-systems/`, `plugins/` | Bản do user thêm; đè bản bundled cùng id |
  | `mcp-config.json`, `mcp-tokens.json`, `mcp-oauth-clients.json` | Cấu hình và credential MCP |

- **Nội dung bundled chỉ đọc**: `skills/`, `design-systems/`, `design-templates/`, `craft/`, `prompt-templates/`, `plugins/_official`, `plugins/registry`, `assets/frames`. Tìm qua `OD_RESOURCE_ROOT` [S `server.ts:1344–1399`].

### 3.2 Project trên đĩa

- **Metadata không nằm trong thư mục project**: nằm trong SQLite (`projects.metadata_json`, kiểu `ProjectMetadata`) [S `packages/contracts/src/api/projects.ts:195`]. Các trường chính:
  - `kind`: prototype, deck, template, image, video, audio, other;
  - `intent`, ví dụ live-artifact;
  - nền tảng, độ chi tiết (fidelity), `entryFile`;
  - `baseDir` (với folder import), `importedFrom`, `linkedDirs`.
- **Bên trong `projects/<id>/`** [S `projects.ts`, `artifacts/manifest.ts`, `project-file-versions.ts`, `live-artifacts/store.ts`]:
  - File của user và agent. Danh sách đệ quy, mới nhất lên đầu; bỏ `node_modules`, file ẩn và các thư mục bị bỏ qua.
  - `<file>.artifact.json`: manifest artifact cho file entry, ẩn khỏi danh sách. Gồm:
    - `kind`: html, deck, react-component, markdown-document, svg, diagram, code-snippet, mini-app, design-system;
    - `renderer`, `exports[]`, `status` (streaming, complete, error), `entry`, `title`, `supportingFiles`;
    - metadata tối đa 16 KB.
  - `.file-versions/<24 ký tự đầu sha256(tên file)>/manifest.json` (schemaVersion 2: fileName, currentVersionId, entries, deletedAt) cộng các file nội dung `NNNN-<id>.<ext>`.
  - `.live-artifacts/<id>/`: `artifact.json`, `template.html`, `data.json`, `index.html`, `provenance.json`, `refreshes.jsonl`, `snapshots/`.
  - `.od-skills/`: skill được stage cho mỗi run.
  - `.transcript.jsonl`: transcript, sinh khi cần.
  - `.file-versions` và `.live-artifacts` là tên dành riêng; file API từ chối đụng tới.
- **Project ở vị trí ngoài**: thêm `.open-design/project.json` (schemaVersion 1, id, name, thời điểm, skillId, designSystemId) [S `project-locations.ts:8–22`].
  - **Đề xuất cho bản Rust**: mọi project đều có file này (đổi tên thư mục ẩn cho repo mới), để project tự mô tả được khi mất SQLite.
- **Folder import**: không bao giờ tạo thư mục gốc; từ chối mọi đường dẫn có segment ẩn; mọi đường dẫn đi qua resolve an toàn [S `projects.ts:129`; D `docs/architecture.md` §6].

### 3.3 SQLite

- **Migration**: `CREATE TABLE IF NOT EXISTS` cộng `ALTER TABLE ADD COLUMN` sau khi kiểm `PRAGMA table_info`. Không dùng `user_version` [S `db.ts:83–691`]. **Bản Rust nên dùng `user_version`** và migration đánh số.
- **Bảng lõi cần có**:

  | Bảng | Nội dung |
  |---|---|
  | `projects` | id, name, skill_id, design_system_id, pending_prompt, metadata_json, custom_instructions, created_at, updated_at |
  | `conversations` | id, project_id (cascade), title, session_mode, các mốc thời gian |
  | `messages` | id, conversation_id, role, content, agent_id, run_id, run_status, last_run_event_id, position, thời điểm; JSON: `events_json` (event đã lưu), attachments, comment_attachments, produced_files, feedback |
  | `message_event_batches` | Event ghi dần trong lúc run; gộp vào `messages.events_json` khi run kết thúc |
  | `agent_sessions` | Khoá (conversation_id, agent_id): session_id, stable_prompt_hash, model, cwd, last_message_id |
  | `templates` | Template do user lưu từ project |
  | `tabs`, `tabs_state` | Tab đang mở theo project |
  | `preview_comments` | selector, position_json, note, status, slide_key, sort_key (bỏ các cột author và cloud) |
  | `media_tasks` | Task sinh media |
  | `routines` | Hẹn giờ (P2) |
  | `installed_plugins`, `plugin_marketplaces`, `applied_plugin_snapshots` | Plugin (P2) |

- **Run không phải bảng**: nằm trong RAM cộng thư mục `runs/<id>/`.
- **Bảng BỎ**: `deployments`, `workspace_*`, `team_*`, khoảng 18 bảng collab, `telemetry_*`, `incidents`, `amr_*`.

### 3.4 Skill và design template

- **Định dạng**: thư mục chứa `SKILL.md` (frontmatter YAML cộng thân Markdown), thêm tuỳ chọn `assets/`, `references/`, script, `example.html`, `examples/<key>.html` [D `docs/skills-protocol.md` §1–2].
- **Key frontmatter**:
  - cấp đầu: `name`, `description`, `triggers`, bản dịch tên và mô tả;
  - trong `od`: `mode`, `surface`, `platform`, `scenario`, `category`, `preview{type, entry}`, `design_system.requires` (mặc định true), `craft.requires`, `example_prompt`, `featured`, `speaker_notes`, `animations`, `upstream`, `upstream_license`, `critique.policy`;
  - `inputs`, `parameters`, `outputs`, `capabilities_required` chỉ có nghĩa với plugin.
- **Parser frontmatter của app gốc là tập con YAML**: scalar, chuỗi khối `|`, list phẳng, map lồng theo thụt dòng; không có flow style và anchor [S `design-systems/frontmatter.ts`]. Bản Rust dùng parser YAML đầy đủ thì **phải test trên toàn bộ file bundled** để chắc kết quả giống.
- **Chuẩn hoá** [S `apps/daemon/src/skills.ts`]:
  - `mode` thuộc 8 giá trị: prototype, deck, template, design-system, image, video, audio, utility. Sai hoặc thiếu thì suy bằng regex trên mô tả và thân theo thứ tự image → video → audio → deck → design-system → template, còn lại là prototype.
  - `platform` mặc định desktop, thành mobile nếu văn bản nhắc mobile, phone, ios, android.
  - `example_prompt` mặc định là câu đầu của `description`, tối đa 320 ký tự.
  - id là `name`, thiếu thì lấy tên thư mục. Có bảng alias cho id đã đổi tên.
  - Mỗi `examples/<key>.html` thành một thẻ con có id `<cha>:<key>`.
  - Quét lại mỗi request; root user trước, root bundled sau, gặp trước thắng.
- **Phân loại** [D `specs/current/skills-and-design-templates.md`]: prototype, deck, template và đầu ra media là design template (`/api/design-templates`). utility và design-system là skill chức năng (`/api/skills`).
- **Hợp đồng deck** [D `design-templates/AGENTS.md`; S `packages/contracts/src/prompts/deck-framework.ts`]:
  - Stage cố định 1920×1080, co giãn theo `min(rộng/1920, cao/1080)`.
  - Mỗi slide là một `section.slide`, chỉ slide `.active` hiện.
  - Phím: →, ↓, PgDn, Space đi tới; ←, ↑, PgUp lùi; Home và End về hai đầu.
  - Nhớ slide hiện tại bằng localStorage.
  - Khi in: `@page` 1920×1080 px, mỗi slide một trang.
  - `<html>` có thuộc tính đánh dấu phiên bản protocol deck.

### 3.5 Design system

- **Package** [D `docs/design-systems.md` §1; S `design-systems/_schema/manifest.schema.ts`]:
  - Bắt buộc: `manifest.json`, `DESIGN.md`, `tokens.css`.
  - Tuỳ chọn: `USAGE.md`, `components.html`, `components.manifest.json`, `design-tokens.json`, `tailwind-v4.css`, `preview/*.html`, `source/*`, `DESIGN-<lang>.md` (17 ngôn ngữ; loader hiện không đọc các file này [S]).
- **Manifest**:
  - `schemaVersion` dạng `…/v1`, `id` trùng tên thư mục, `name`, `category`, `description`;
  - `source{type: bundled|local|github|shadcn, origin}`;
  - `files{…}`, `importMode` (normalized, hybrid, verbatim);
  - `craft{applies, suggested, exemptions}`, `preview{dir, pages[]}`.
- **`DESIGN.md`**: văn xuôi tự do, ít nhất 7 mục H2.
- **Token**, chia theo lớp [S `packages/contracts/src/design-systems/token-schema.ts`]:

  | Lớp | Nội dung |
  |---|---|
  | A1 identity | nền, bề mặt, chữ, màu phụ, viền, accent, font display, font body |
  | A1 structure | thang cỡ chữ xs–4xl, leading, tracking, container, khoảng cách section |
  | A2 (có giá trị fallback) | trạng thái accent, danger/warn/success, focus ring, font mono, radius, space 1–12, elevation, easing, thời lượng motion |
  | B (tuỳ chọn) | các slot phụ |

- **Thứ tự fallback khi liệt kê**: tiêu đề lấy metadata user → manifest → H1 → frontmatter → tên thư mục. Category, summary và swatch có chuỗi fallback tương tự [S `design-systems/index.ts:~320`].

### 3.6 Craft, prompt template, plugin

- **Craft**: 11 file `craft/<slug>.md`. Tập slug cần nạp = `craft.requires` của skill ∪ `craft.applies` của design system ∪ `typography` (nếu là deck), trừ `exemptions`. Mỗi file thành một mục `### <slug>`, nối bằng đường kẻ ngang. File thiếu thì bỏ qua, không báo lỗi [S `craft.ts`].
- **Prompt template**: `prompt-templates/{image,video}/*.json` với các trường id, surface, title, summary, category, tags, model, aspect, prompt, ảnh preview, source (có `license`) [S `media/prompt-templates.ts`].
- **Plugin** [D `docs/plugins-spec.md`; schema `docs/schemas/open-design.plugin.v1.json`]:
  - Thư mục có `SKILL.md` (hoặc `.claude-plugin/plugin.json`), cộng `open-design.json` tuỳ chọn.
  - Trường cấp đầu: name, title, version, description, license, tags, compat.
  - Khối `od`: `kind` (skill, scenario, atom, bundle), mode, `preview`, `useCase.query` (có biến `{{…}}`), `context{skills, designSystem, craft, mcp, atoms}`, `pipeline.stages[{atoms, repeat, until}]`, `genui.surfaces`, `connectors`, `inputs`, `capabilities`.
  - Capability: `prompt:inject`, `fs:read`, `fs:write`, `mcp`, `subprocess`, `bash`, `network`, `connector[:id]`. Trust: restricted hoặc trusted. Thiếu capability thì HTTP 409 hoặc exit 66.
  - Marketplace: `plugins/registry/*/open-design-marketplace.json`, khoảng 420 entry trỏ về GitHub.

### 3.7 Tái dùng nội dung theo license

- Repo gốc là Apache-2.0, **không có file NOTICE ở gốc** [S]. Nhưng trong các thư mục nội dung có **102 file LICENSE riêng**, phần lớn MIT; một phần prompt là CC-BY-4.0; có font kèm và không kèm license [S, đếm bằng grep].
- **Khuyến nghị**: repo đích tạo `NOTICE` ghi nguồn app gốc, và **giữ nguyên mọi file LICENSE con** khi copy.

| Thư mục | Số lượng | Tái dùng nguyên được? | Ghi chú |
|---|---|---|---|
| `craft/` | 11 file | Được | Phỏng theo nguồn MIT (đã ghi trong `craft/README.md`); giữ README |
| `design-templates/` | khoảng 115 thư mục | Được, giữ LICENSE con | 37 file LICENSE MIT của bên thứ ba (nhiều bộ deck); `hyperframes/` fork từ nguồn Apache-2.0 |
| `skills/` | khoảng 170 thư mục | Được, giữ LICENSE con | 36 LICENSE MIT. Có 194 skill khai báo `upstream` nhưng chỉ 32 khai báo `upstream_license`: Atlat soát nốt trước khi phát hành |
| `design-systems/` | khoảng 152 package + `_schema` | Được về license, **rủi ro thương hiệu** | Nhiều package đặt theo tên thương hiệu thật. Nên đổi tên hiển thị hoặc chỉ ship bộ trung tính, và hỏi anh. Attribution có trong `design-systems/README.md` |
| `prompt-templates/` | 107 JSON | **Có điều kiện** | 72 file CC-BY-4.0 (cần ghi công), 31 Apache, 1 MIT, 2 file không rõ license (loại ra). Ảnh preview là URL CDN bên thứ ba |
| `plugins/_official/atoms`, `scenarios` | 13 + 14 | Được | Bỏ scenario cloud và rollout (`od-next-strategy` chỉ lấy nếu chọn prompt bản Next) |
| `plugins/_official/design-systems` | 143 | Không cần | Bản sao byte-giống `design-systems/`; manifest ghi MIT, lệch với Apache gốc |
| `plugins/_official/examples` | 188 | Được, giữ LICENSE con | Có font kèm OFL; một số font **không có file license** (`examples/open-design-homepage/assets/fonts`): loại ra |
| `plugins/_official/image-templates`, `video-templates` | 45 + 63 | Có điều kiện | CC-BY-4.0 chiếm phần lớn. Font hash trong `video-templates/*/source/fonts` không có license: loại ra hoặc soát |
| `plugins/community/` | 10 | Soát từng cái | |
| `templates/` | 10 file | Được | Runtime app gốc không nạp thư mục này |
| `apps/daemon/tests/fixtures/claude-cli-recordings/` | | Được (dữ liệu test) | Dùng làm golden test cho parser |

---

## 4. Preview và export

### 4.1 Preview HTML

**Chọn renderer theo manifest** [S `apps/web/src/artifacts/renderer-registry.ts`]: react-component → deck-html → html → markdown → svg. File còn lại đi theo loại file (mục 1.2).

**Hai cách nạp** [S `components/file-viewer-render-mode.ts:104–137`]:
- **Theo URL** (mặc định cho artifact nhiều file): iframe `src` = `/api/projects/:id/raw/<path>?v=<mtime>&…`. Từng asset tải riêng, cache được.
- **srcDoc**: host dựng sẵn một tài liệu đã chèn bridge. Bắt buộc dùng srcDoc khi:
  - là deck;
  - bật inspect, edit, palette hoặc tweaks;
  - có form hoặc hash router;
  - HTML tham chiếu file bằng đường dẫn bắt đầu từ `/`;
  - bật comment hoặc draw mà URL không có bridge tương ứng.
- **Heuristic để chọn**: so chuỗi thô, không parse HTML. Ví dụ có `localStorage` thì cần shim Web Storage; có `SharedArrayBuffer`, `.wasm`, WebGL2, `navigator.gpu` thì cần "powered preview" (cross-origin isolated).
- Host giữ cả hai iframe cùng lúc và chỉ đổi CSS visibility, để chuyển chế độ không bị nháy [D `AGENTS.md` Chat UI].

**Sandbox iframe** [S `FileViewer.tsx:437–448`]:
- Thường: `allow-scripts allow-forms allow-downloads`, origin opaque.
- Powered: thêm `allow-same-origin` và chạy trên host riêng mỗi project (`od-<hash>.localhost`).

**Daemon serve preview** [S `routes/project/index.ts:5950, 6454, 6846, 7014, 7093, 7233`]:
- `raw`: ETag và Last-Modified. Với HTML ≤ 2 MiB, nếu query yêu cầu bridge thì chèn script bridge trước script của tác giả, và chèn `<base href>` trỏ tới scope preview.
- `preview-url`: cấp scope token ngắn hạn kèm CSP và sandbox. `preview/<scope>/…` serve với CSP chặt: chỉ `'self'`, `data:`, `blob:`; `connect-src 'none'`; `form-action 'none'`; `base-uri 'none'`.
- `powered`: header `Document-Isolation-Policy`, không có CSP.

**Bridge**: script chèn vào trang, nói chuyện với host bằng `postMessage`, type có tiền tố `od:` [S `runtime/srcdoc.ts`; `packages/contracts/src/runtime/deck-protocol.ts`]:

| Bridge | Việc |
|---|---|
| Deck | Điều khiển next, prev, first, last, đi tới slide i; báo trạng thái (slide active, tổng số); báo deck sẵn sàng; báo Esc |
| Selection | Hover và chọn phần tử cho comment và inspect; định danh phần tử bằng `data-od-id` gắn sẵn |
| Inspect | Đặt, reset, replay, trích override style |
| Palette, tweaks, edit | Đổi màu, hiện panel tham số, sửa text |
| Snapshot | Chụp **không cần screenshot thật**: clone DOM, chép computed style, vẽ qua SVG foreignObject |
| Export-capture | Chờ font, ảnh và 2 animation frame; duyệt từng slide |
| Guard | Chống vòng lặp redirect (luôn bật), chặn cướp focus, shim Web Storage, đo kích thước nội dung |

**Live reload** [S `project-watchers.ts`, `providers/project-events.ts`]:
- Mỗi project một watcher, đếm số người theo dõi; tạo khi có người đầu tiên, đóng khi người cuối rời.
- Đợi file ổn định 200 ms. Bỏ symlink và file rác. Hết handle watch thì chuyển sang polling.
- Event qua SSE project. Client reconnect backoff 1–30 s; nhận event thì làm mới danh sách file, `?v=` đổi làm iframe nạp lại.
- **Tiêu chí bản Rust**: ghi file → preview cập nhật trong ≤ 500 ms (chưa tính thời gian render của webview).

### 4.2 Export: cái gì cần browser engine

| Năng lực | Cần browser engine? | Lý do | App gốc dùng |
|---|---|---|---|
| Preview tương tác, bridge | Có | Chạy JS và layout của artifact | Iframe trong renderer |
| Serve file, CSP, chèn bridge, `<base>` | Không | Viết lại header và chuỗi | Express |
| Live reload | Không | Watcher cộng SSE | chokidar |
| HTML standalone | Không | Đi theo đồ thị phụ thuộc, nhúng `data:`, module thành importmap. Giới hạn: entry 2 MiB, 2.500 node, 50 MiB mỗi asset, đầu ra 100 MiB; vượt thì 413 | cheerio, postcss, babel [S `artifacts/standalone-html.ts`] |
| ZIP | Không | Stream archive | [S] |
| Transcript JSONL | Không | Đọc SQLite, ghi file tạm rồi rename, có lock | [S `transcript-export.ts`] |
| PDF vector | Có | `@page`, layout in | Electron `printToPDF`; trang deck 13,333×7,5 in [S `desktop/src/main/pdf-export.ts`] |
| PDF raster (mặc định, kể cả CLI) | Có | Chụp từng slide rồi ghép; app gốc chọn cách này vì `printToPDF` mất glyph CJK | CDP `Page.captureScreenshot` + pdf-lib [S `export-cli-routing.ts`, `deck-export.ts`] |
| PPTX ảnh | Có (để chụp) | Mỗi slide một ảnh tràn khung, rộng 13,333 in | pptxgenjs [S] |
| PPTX editable | Có | Đo layout DOM thật | dom-to-pptx [S] |
| Ảnh PNG/JPEG | Có | Một slide, deck ghép dọc, hoặc trang dài tới 20.000 px | CDP hoặc `capturePage` [S] |
| Ảnh bìa project | Có | Render khung đầu 1440×900; ngân sách 5 s tài nguyên, 8 s tổng | Cửa sổ ẩn [S] |
| MP4 HyperFrames | Có, cộng ffmpeg | Phải chạy timeline GSAP rồi seek từng frame | CDP chụp frame + ffmpeg (xem dưới) [S] |
| Ảnh bìa video | Không | Lấy frame đầu | ffmpeg |
| Sinh media (ảnh, video, audio) | Không | Gọi HTTP tới provider | fetch [S `media/`] |
| Live artifact render và refresh | Không | Thay template `{{data.x}}`, lặp `data-od-repeat`; từ chối template có script hay iframe | TS thuần [S `live-artifacts/render.ts`] |
| Lint, kiểm deliverable | Không | Regex, AST, kiểm filesystem | [S] |
| Xem trước PDF, DOCX, PPTX, XLSX | Không | Trích text: `pdftotext` ngoài; DOCX, PPTX, XLSX giải zip rồi đọc XML | [S `document-preview.ts`] |

**Pipeline MP4 HyperFrames** [S `media/index.ts:3863–4200`, `media/hyperframes-runtime.ts`, `desktop/src/main/frame-capture.ts`]:
1. Agent tạo composition trong `.hyperframes-cache/<id>`, gồm `hyperframes.json`, `meta.json` và `index.html`. Trang phải đăng ký `window.__timelines`.
2. Đọc width, height, fps (mặc định 30) từ phần tử có `data-composition-id`.
3. Chèn runtime HyperFrames (thư viện Apache-2.0 bên ngoài) và bridge frame.
4. Với mỗi frame i: seek tới t = i/fps, chụp PNG.
5. ffmpeg: libx264, `yuv420p`, `+faststart`, pad kích thước về số chẵn, timeout 5 phút.

**Ranh giới cho bản Rust**: mọi việc cần browser engine đi qua **một trait `Renderer`** với 4 thao tác:
- `render_slides`: deck → danh sách PNG;
- `render_frames`: timeline → PNG theo fps;
- `print_pdf`;
- `capture_page`.

Daemon không cần biết bên dưới là gì (xem 5.3).

---

## 5. Đề xuất kiến trúc Rust

### 5.1 Nguyên tắc

- **Giữ ranh giới daemon ↔ UI bằng HTTP và SSE** như app gốc. Lý do: CLI `od`, MCP server và UI dùng chung một API, giữ được luật dual-track; chạy headless và Docker không phải sửa gì.
- **UI và daemon chung một process khi chạy desktop**: axum bind loopback port ngẫu nhiên, webview trỏ vào. Không cần sidecar IPC, không cần HMAC.
- **Chỉ một bộ prompt** (legacy). Không có rollout, không có analytics.
- **Không có mạng ra ngoài**, trừ gọi agent hoặc provider do user cấu hình. Không có telemetry, không có auto-update ngầm.

### 5.2 Crate layout (Cargo workspace)

```
open-design-rs/
  crates/
    od-contracts/   DTO serde: ChatRequest, AgentEvent (union mục 2.2), Project, ArtifactManifest…; xuất JSON Schema để test
    od-store/       SQLite (rusqlite, WAL, migration theo user_version) + layout thư mục + file version + path an toàn
    od-content/     Nạp và index skill, design template, design system, craft, prompt template, plugin; parse frontmatter; ghép prompt
    od-runtime/     RuntimeDef (dữ liệu), detection, spawn (process group), parser từng stream format, ACP client, cancel/resume/steer, retry, loop guard
    od-runs/        Run registry: ring buffer, events.jsonl, state.json, fan-out SSE, replay
    od-preview/     Serve file preview, CSP, chèn bridge, watcher (notify) → event project
    od-export/      standalone HTML, ZIP, transcript; trait Renderer + PDF/PPTX ghép (printpdf, crate pptx tự viết theo OOXML tối giản)
    od-render-cdp/  Renderer qua Chrome DevTools Protocol (chromiumoxide), dùng Chrome/Edge có sẵn trên máy; ffmpeg cho MP4
    od-media/       (P2) adapter provider media
    od-server/      axum: route /api/*, middleware origin/token, SSE
    od-mcp/         MCP stdio server (rmcp) proxy về HTTP; od mcp install
    od-cli/         binary `od` (clap): daemon + subcommand client
    od-ui/          Frontend (Leptos CSR → WASM), build ra static, daemon serve
  apps/
    desktop/        Tauri 2 shell: cửa sổ, dialog native, mở link, nhúng od-server trong process
  bridges/          JS bridge chèn vào preview (deck, selection, snapshot, guard) — viết mới, không chép
  content/          skills/, design-templates/, design-systems/, craft/ … copy từ app gốc theo mục 3.7, kèm NOTICE
```

### 5.3 Chọn UI

Ràng buộc gốc: **preview bắt buộc là web engine thật**, vì artifact là HTML, CSS, JS do agent viết. Lựa chọn thật sự chỉ là UI của app có chạy trong webview luôn hay không.

| Phương án | Ưu | Nhược | Kết luận |
|---|---|---|---|
| **Tauri 2 + Leptos (CSR/WASM)** | Preview là iframe ngay trong cùng webview, bridge `postMessage` giữ nguyên mô hình. UI có thể mở bằng browser thường khi chạy headless. Leptos có fine-grained reactivity, stream chat nhanh. Tauri lo dialog, menu, bundle | WASM tăng kích thước bundle khoảng 1–2 MB. Webview mỗi OS một engine (WKWebView, WebView2, WebKitGTK) nên preview khác Chrome đôi chút | **Chọn** |
| Tauri 2 + Dioxus (web) | Tương tự, API giống React, dễ cho người quen React | Hệ sinh thái component nhỏ hơn Leptos cho SPA thuần web; Dioxus desktop native vẫn là webview | Thay thế được, ngang Leptos |
| GPUI + wry cho preview | UI native rất nhanh (kiểu Zed) | GPUI chưa ổn định ngoài Zed, tài liệu ít. Nhúng child webview vào GPUI phải tự làm. Mất chế độ UI qua browser | Không chọn |
| egui + wry | Đơn giản, nhẹ | Text, i18n, RTL và Markdown phong phú khó làm. Phải tự đồng bộ vị trí child webview, z-order lỗi. Mất UI qua browser | Không chọn |

**Export cần pixel giống Chrome**: không dựa vào webview của OS. Dùng `od-render-cdp` (chromiumoxide) điều khiển Chrome, Edge hoặc Chromium có sẵn trên máy, chạy headless.
- Không có Chrome thì: PDF rơi về in của webview (WKWebView có `createPDF`, WebView2 có `PrintToPdf`); chụp slide báo "cần Chrome".
- Đây là [Suy], cần anh quyết có cho phép tải Chromium về không (mục 5.6).

### 5.4 Runtime và lưu trữ

- **tokio** đa luồng. Mỗi run có:
  - 1 task đọc stdout theo dòng (`tokio::io::BufReader::lines`, giới hạn độ dài dòng);
  - 1 task đọc stderr;
  - 1 task watchdog cho timeout.
- **Spawn**: `tokio::process::Command`. POSIX dùng `process_group(0)` rồi `killpg`; Windows dùng Job Object để kill cả cây.
- **Event bus**: `tokio::sync::broadcast` cho live, cộng `VecDeque` 2.000 event làm ring. Ghi `events.jsonl` qua một writer có buffer, flush mỗi 50 ms hoặc khi gặp event lifecycle. `state.json` ghi đồng bộ, theo kiểu ghi file tạm rồi rename.
- **SQLite**: rusqlite, WAL, `synchronous=NORMAL`. Một thread writer riêng nhận lệnh qua channel; nhiều connection chỉ đọc. Event của message ghi theo lô (giống `message_event_batches`) rồi gộp khi kết thúc run.
- **Content index**: quét một lần lúc khởi động (song song bằng rayon), giữ trong `ArcSwap`. Theo dõi các root user bằng `notify` để làm mới; không quét lại mỗi request như app gốc.
- **File serving**: tower-http `ServeFile` có ETag và Range. Chèn bridge bằng `lol_html` (streaming rewriter), không parse cả DOM.
- **Watcher**: `notify` + `notify-debouncer-full`, cửa sổ 200 ms; mỗi project một watcher, đếm tham chiếu.

### 5.5 Chiến lược hiệu năng (mục tiêu đo được)

| Chỉ số | Mục tiêu | Cách đo |
|---|---|---|
| Khởi động lạnh tới UI dùng được | ≤ 800 ms trên M-series | Đo từ lúc tạo process tới khi `GET /api/health` cộng first paint |
| Index toàn bộ content bundled (khoảng 450 skill, template, system) | ≤ 300 ms | Bench `od-content` |
| Độ trễ từ dòng stdout của agent tới event SSE ở client | p99 ≤ 20 ms | Mock CLI phát timestamp |
| Ghi file tới event `file-changed` | ≤ 300 ms | Test tích hợp |
| RAM daemon lúc rảnh | ≤ 80 MB (không tính webview) | |
| Replay 50.000 event từ jsonl | ≤ 500 ms | Bench |

### 5.6 Câu hỏi cần anh quyết (ảnh hưởng thiết kế)

1. **Quyền của agent**: app gốc chạy Claude với `bypassPermissions` và nhiều CLI với cờ tự duyệt (`--yolo`, `--force`, `--dangerously-…`). Bản Rust giữ mặc định đó, hay thêm chế độ "hỏi trước"?
2. **Chrome cho export**: chỉ dùng Chrome có sẵn, hay cho phép tải Chromium (khoảng 150 MB) khi cần?
3. **Design system mang tên thương hiệu thật**: ship nguyên, đổi tên, hay chỉ ship bộ trung tính?
4. **Bộ prompt**: bám bản legacy (đề xuất), hay bản "Next" dạng plugin markdown?

### 5.7 Kế hoạch 6 mốc cho builder

Mỗi mốc có tiêu chí xong kiểm được bằng `cargo test` hoặc test e2e. Mock CLI: viết lại theo ý tưởng `mocks/` của app gốc. Đó là script phát lại file JSONL ghi sẵn, đặt lên đầu PATH.

**M1: Nền dữ liệu và nội dung** (khoảng 1–1,5 tuần)
- Phạm vi: `od-contracts`, `od-store`, `od-content`, `od-server` (health, projects, files, skills, design-templates, design-systems, craft), CLI `od project`, `od files`.
- Xong khi:
  - Nạp toàn bộ `content/` không lỗi. Số mục khớp số thư mục (test so sánh với `ls`). Mọi `SKILL.md` parse ra `mode` thuộc 8 giá trị.
  - Project CRUD đi vòng HTTP được. `projects/<id>/` và `.open-design/project.json` được tạo đúng.
  - Path traversal (`../`, symlink ra ngoài, segment ẩn ở project import) đều trả 400 hoặc 403 (bảng test ≥ 10 ca).
  - `GET /raw/<path>` có ETag; gửi lại với `If-None-Match` thì nhận 304.
  - Tạo version, liệt kê, restore: nội dung trở về đúng byte.

**M2: Run engine với Claude Code và Codex** (khoảng 2 tuần)
- Phạm vi: `od-runtime`, `od-runs`, `POST /api/runs`, SSE events, cancel, resume, ghép prompt legacy, `od run`.
- Xong khi:
  - Golden test: phát lại các bản ghi stream Claude (fixture app gốc) qua parser, ra đúng chuỗi event chuẩn đã chốt. Mỗi lượt có đúng 1 lần `turn_end`; frame sub-agent không kích hoạt nó.
  - Mock Claude chạy hai lượt: lượt 2 có `--resume <id lượt 1>`. Đổi model thì không resume.
  - Ngắt SSE ở event k, nối lại bằng `Last-Event-ID`: nhận đủ từ k+1, không trùng. Restart daemon giữa run: run có `end{failed}`.
  - Cancel: cả cây tiến trình mock (có spawn con) chết trong ≤ 3,5 s; `end{canceled}`.
  - Steer khi stdin đã đóng trả 409 `stdin_closed`.
  - Codex exec-json: fixture `command_execution` ra cặp `tool_use` và `tool_result` tên Bash.
  - `POST /api/runs` gửi lại cùng `clientRequestId` thì trả `reused: true`.

**M3: UI MVP và live preview** (khoảng 2 tuần)
- Phạm vi: `od-ui` (Leptos), Tauri shell, `od-preview`. Gồm:
  - Home (ô brief, chip prototype và deck, chọn design system và template);
  - danh sách project;
  - Studio (chat stream, stop, question-form, cây file);
  - preview iframe URL-load cộng watcher;
  - onboarding chọn agent;
  - Settings agent và model.
- Xong khi (e2e bằng WebDriver/tauri-driver hoặc Playwright trên bản headless):
  - Kịch bản vàng: nhập brief → mock agent ghi `index.html` → preview hiện nội dung đó. Agent ghi đè file → preview đổi trong ≤ 1 s. Gõ lượt 2 → agent nhận được prompt có transcript hoặc resume.
  - Question-form trong stream hiện thành form; nộp form thì tạo tin nhắn user có câu trả lời đã format.
  - Không có request mạng nào ra ngoài loopback trong suốt kịch bản (kiểm bằng proxy chặn).

**M4: Deck, bridge và sửa trên preview** (khoảng 1,5 tuần)
- Phạm vi: chế độ srcDoc cộng bridge viết mới (deck, selection/comment, snapshot, các guard), deck viewer (phím, thumbnail, present), comment gửi kèm lượt sau, lịch sử version trong UI.
- Xong khi:
  - Với deck mẫu từ `design-templates/`: lệnh next, prev, go(i) làm trạng thái slide trả về đúng index; Home và End về hai đầu.
  - Click phần tử trong comment mode tạo comment có selector ổn định. Reload preview thì comment vẫn bám đúng phần tử.
  - Trang có `location.reload()` vòng lặp không làm treo host (redirect guard).

**M5: Export, CLI và MCP** (khoảng 2 tuần)
- Phạm vi: ZIP, HTML standalone, transcript, `od-render-cdp` (PDF raster và vector, PNG, PPTX ảnh), `od export`, `od mcp` (20 tool, resource, active project), `od mcp install` (claude, codex, cursor trước).
- Xong khi:
  - HTML standalone mở được khi **tắt mạng**, không có request tới đường dẫn tương đối nào (test đọc lại, không còn URL tương đối cục bộ). Vượt giới hạn trả 413.
  - Deck 5 slide → PDF 5 trang và PPTX 5 slide (đọc lại bằng parser, đếm số trang và slide). PNG đúng kích thước 1920×1080.
  - MCP: client test qua stdio gọi `list_projects`, `write_file`, `get_file`. Bỏ trống `project` thì dùng active project; quá 5 phút thì báo lỗi rõ ràng.
  - `od mcp install cursor --print` in ra JSON hợp lệ. Cài thật thì merge vào config, không xoá entry khác (test với file config giả).

**M6: Mở rộng (P1 và P2)** (khoảng 2–3 tuần, chia nhỏ được)
- Phạm vi: ACP client chung (Kimi, Hermes…), OpenCode, BYOK, plugin bundled làm preset, media gen (OpenAI, fal, ElevenLabs), HyperFrames MP4, live artifact, i18n (en, vi).
- Xong khi:
  - ACP: mock agent ACP đi đủ `initialize` → `session/new` → `session/prompt`. Yêu cầu permission được tự duyệt theo cấu hình. Cancel gửi `session/cancel` trước khi kill.
  - HyperFrames: composition mẫu 2 s, 30 fps → MP4 có 60 frame (đếm bằng ffprobe), mã hoá h264 với `yuv420p`.
  - Live artifact: template có `<script>` bị từ chối. Refresh cập nhật `data.json` và ghi một dòng vào `refreshes.jsonl`.
