# open-design-rs

Bản viết lại [OpenDesign](https://github.com/nexu-io/open-design) (Apache-2.0) bằng Rust. App chạy hoàn toàn trên máy: không đăng nhập, không cloud, không telemetry. Một coding agent cài sẵn trên máy (Claude Code, Codex…) viết thiết kế HTML vào thư mục project; app hiển thị preview, lịch sử phiên bản và xuất file.

Có hai cách chạy, dùng chung một HTTP API:

- **Daemon + trình duyệt**: binary `od` chạy daemon tại `http://127.0.0.1:7456`, UI mở trong trình duyệt.
- **App desktop** (Tauri 2): daemon chạy ngay trong process của app, cửa sổ trỏ vào nó.

Kiến trúc tóm tắt ở [`docs/architecture.md`](docs/architecture.md). Spec hành vi: [`docs/spec.md`](docs/spec.md). Quyết định đã chốt: [`docs/decisions.md`](docs/decisions.md).

## Cài đặt

Cần có:

| Công cụ | Dùng cho | Cài |
|---|---|---|
| Rust 1.85+ (edition 2024) | build mọi thứ | `rustup` ([rustup.rs](https://rustup.rs)) |
| target `wasm32-unknown-unknown` | build UI | `rustup target add wasm32-unknown-unknown` |
| `trunk` | đóng gói UI (Leptos → WASM) | `cargo install trunk --locked` |
| Node.js 20+ | e2e và Tauri CLI | nodejs.org hoặc `brew install node` |
| Chrome, Edge hoặc Chromium | export PDF, ảnh, PPTX, MP4 | dùng bản có sẵn trên máy; app không tự tải Chromium |
| `ffmpeg` (tuỳ chọn) | export MP4 | `brew install ffmpeg` |

Ít nhất một agent CLI (xem [Agent](#agent)) hoặc một API key (xem [BYOK](#byok-dùng-api-key)).

Build UI một lần (ra `crates/od-ui/dist`, daemon và app desktop đều phục vụ thư mục này):

```bash
make ui-dist
```

## Chạy daemon và UI

```bash
cargo run -p od-cli --bin od                     # daemon tại http://127.0.0.1:7456
```

Mở `http://127.0.0.1:7456` trong trình duyệt. Lần đầu UI vào màn hình chọn agent, sau đó là Home: gõ brief, chọn loại (prototype, slide deck), design system và template rồi bấm Create.

Chạy daemon ở port khác, kiểm tra daemon đang chạy:

```bash
cargo run -p od-cli --bin od -- daemon start --port 7460
cargo run -p od-cli --bin od -- --daemon-url http://127.0.0.1:7460 daemon status
```

Mỗi thư mục dữ liệu chỉ có một daemon: daemon thứ hai cùng `OD_DATA_DIR` báo lỗi và thoát. Dữ liệu mặc định nằm ở `~/Library/Application Support/open-design-rs` (macOS) hoặc `~/.local/share/open-design-rs` (Linux).

Sửa UI có hot reload: chạy daemon ở port 7456 như trên, rồi ở terminal khác

```bash
cd crates/od-ui && trunk serve                   # http://127.0.0.1:8080, /api/ chuyển tới daemon
```

## App desktop

Chạy thử từ source (cần `make ui-dist` trước):

```bash
cargo run -p od-desktop
```

Build app `.app` (macOS) bằng Tauri CLI, bản debug khoảng 3–5 phút lần đầu:

```bash
make ui-dist
cd apps/desktop && npx --yes @tauri-apps/cli@2 build --debug
open "$(cargo metadata --format-version 1 --no-deps | node -pe 'JSON.parse(require("fs").readFileSync(0)).target_directory')/debug/bundle/macos/Open Design.app"
```

Bỏ `--debug` để build bản release (lâu hơn, file nhỏ hơn). Bundle chứa sẵn `content/` và UI.

Hành vi của app desktop:

- Daemon chạy trong process của app, ở một port loopback ngẫu nhiên mỗi lần mở. Không có sidecar.
- Dùng chung thư mục dữ liệu với `od`. Đang có daemon `od` chạy thì app báo lỗi "đã có daemon" và thoát; tắt `od` trước.
- Menu **Project ▸ Open Folder…** (Cmd+O) mở một thư mục có sẵn thành project.
- Mở từ Finder hoặc Dock, app lấy `PATH` từ login shell (cộng `~/.local/bin`, Homebrew, npm, bun, cargo) để tìm agent CLI.
- Đóng app thì shell của các tab terminal và agent đang chạy đều dừng theo.

## Agent

App điều khiển agent CLI có sẵn trên máy, không gọi model trực tiếp (trừ BYOK). Agent hỗ trợ:

| Agent | Lệnh tìm trên `PATH` |
|---|---|
| Claude Code | `claude` |
| Codex CLI | `codex` |
| Cursor Agent | `cursor-agent` |
| GitHub Copilot CLI | `copilot` |
| OpenCode | `opencode-cli`, rồi `opencode` |
| Qwen Code | `qwen` |

Cùng 10 agent theo giao thức ACP: Kimi CLI (`kimi`), Hermes (`hermes`), Kiro CLI (`kiro-cli`), Kilo (`kilo`), Mistral Vibe (`vibe-acp`), Trae CLI (`traecli`), DeepSeek Reasonix (`reasonix`), Devin for Terminal (`devin`), DeepSeek Harness (`dsh`, rồi `dsnix`).

Cách phát hiện: mỗi lần mở màn hình chọn agent hoặc gọi `GET /api/agents`, daemon tìm binary trên `PATH` của nó, chạy `--version` rồi đọc `--help` để biết cờ nào được hỗ trợ (mỗi lệnh tối đa 5 giây). Không cache xuống đĩa: cài agent xong chỉ cần bấm Refresh.

```bash
curl -s http://127.0.0.1:7456/api/agents
```

Agent chạy với thư mục làm việc là thư mục project. Daemon xoá mọi biến `OD_*` thừa hưởng (kể cả `OD_API_TOKEN`) rồi đặt các biến của run: `OD_PROJECT_DIR`, `OD_DAEMON_URL`, `OD_TOOL_TOKEN`, `OD_BIN`…

### Quyền của agent

Mặc định là **Tự động** (giống app gốc): Claude chạy với `--permission-mode bypassPermissions`, agent khác kèm cờ tự duyệt (`--yolo`, `--dangerously-skip-permissions`…). Chuyển sang **Hỏi trước** trong Settings ▸ Permissions, hoặc qua API:

```bash
curl -s -X PUT http://127.0.0.1:7456/api/app-config -H 'content-type: application/json' -d '{"agentPermissionMode":"ask"}'
```

Ở chế độ hỏi trước, Claude chỉ được sửa file (`acceptEdits`), agent khác chạy không kèm cờ tự duyệt. Agent nào chờ người duyệt mà không có ai trả lời sẽ treo tới khi hết thời gian chờ; khi đó chuyển lại Tự động.

### BYOK (dùng API key)

Không có agent CLI thì dùng agent "API key (BYOK)": daemon tự gọi Anthropic Messages API hoặc một endpoint tương thích OpenAI `chat/completions`. Model không có tool, chỉ trả về một file HTML.

```bash
curl -s -X PUT http://127.0.0.1:7456/api/app-config -H 'content-type: application/json' \
  -d '{"byok":{"protocol":"anthropic","apiKey":"sk-ant-...","model":"claude-opus-5-5"}}'
```

- `protocol`: `anthropic` hoặc `openai` (`openai-compatible` cũng được).
- `baseUrl` tuỳ chọn, cho endpoint tự host. `maxTokens` tuỳ chọn.
- Key lưu trong `<OD_DATA_DIR>/app-config.json` (quyền `0600`), API không bao giờ trả key ra, chỉ gửi tới endpoint đã cấu hình.

## Dùng CLI

Mọi lệnh trừ `od` / `od daemon start` là client mỏng gọi HTTP API; thêm `--json` để in JSON, `--daemon-url` để chọn daemon.

```bash
od project create --name "Landing"                         # in ra <project-id>
od files write <project-id> index.html --content "<h1>Hi</h1>"
od files list <project-id>
od run start --project <project-id> --agent claude --message "Làm landing page cho tiệm trà" --watch
od run cancel <run-id>
od lint <project-id> [file] [--fail-on p0|p1|p2|none] [--agent-message]
od export <project-id> index.html --format pdf
```

Chạy từ source thì thay `od` bằng `cargo run -p od-cli --bin od --`.

- `od run watch <run-id> [--after N]` in event của run dạng NDJSON (`{id, event, data}`), thoát 0 khi run `succeeded`.
- `od lint` chạy bộ lint "AI slop" (gradient tím, emoji làm icon, màu indigo mặc định…) trên một file HTML, hoặc mọi file HTML của project khi bỏ trống `file`. Thoát 1 khi có lỗi từ mức `--fail-on` trở lên (mặc định `p0`).
- `od media generate|wait|cancel`, `od plugin list|info|apply|validate`: xem `od <lệnh> --help`. `od plugin apply` thoát 66 khi plugin thiếu capability.

## Export

Trong Studio: nút Export trên preview. Từ CLI: `od export <project-id> [file] --format <định dạng>` (mặc định ghi vào thư mục hiện tại; `--out -` ghi ra stdout).

| `--format` | Kết quả | Cần |
|---|---|---|
| `html` | một file HTML tự chứa (CSS, ảnh, font nhúng sẵn) | — |
| `zip` | toàn bộ project (hoặc `--root <thư-mục>`) | — |
| `pdf` | PDF raster, mỗi slide một trang | Chrome |
| `pdf-vector` | PDF vector qua pipeline in | Chrome |
| `image` | PNG/JPEG: một slide (`--index`), cả deck ghép dọc, hoặc cả trang | Chrome |
| `pptx` | PowerPoint, mỗi slide một ảnh | Chrome |
| `pptx-editable` | PowerPoint với text box và shape gốc | Chrome |
| `mp4` | video từ composition HyperFrames | Chrome, `ffmpeg` |

Chrome được tìm ở đường dẫn chuẩn của Chrome, Edge, Chromium, hoặc `OD_CHROME_PATH`. Không có Chrome thì các định dạng cần Chrome trả lỗi `this export needs Chrome, Edge or Chromium`.

## Terminal

Trong Studio, nút `>_` mở một tab terminal: shell của người dùng (hoặc `OD_TERMINAL_SHELL`) chạy trong thư mục project, hiển thị bằng xterm.js đóng gói sẵn trong UI (không tải từ CDN). Chương trình toàn màn hình như `vim`, `top`, `htop` hiển thị đúng; kích thước terminal theo khung tab.

- Chuyển sang tab khác hay tải lại trang, shell vẫn chạy; mở lại tab thì daemon phát lại scrollback.
- Đóng tab thì cả cây tiến trình của shell bị dừng.
- API: `POST /api/projects/:id/terminals`, WebSocket `GET /api/terminals/:tid/ws` (frame binary hai chiều, `{"type":"resize","cols","rows"}`), `DELETE /api/terminals/:tid`.

## Routines (tự động hoá)

Trang **Automations**: một routine chạy một prompt trong một project theo lịch cron giờ địa phương, hoặc bấm Run now. Mỗi lần chạy là một run agent bình thường trong một conversation mới mang tên routine, nên xem lại được trong Studio.

- Lịch: cron 5 trường (`phút giờ ngày tháng thứ`, có `*`, danh sách, khoảng, bước) hoặc `@hourly`, `@daily`, `@weekly`, `@monthly`.
- Một routine không bao giờ chạy chồng: tới giờ mà lần trước chưa xong thì lần này ghi là `skipped`.
- Daemon tắt thì lịch không chạy bù; lần kế tiếp tính từ lúc daemon chạy lại.
- API: `GET/POST /api/routines`, `POST /api/routines/:rid/run`, `GET /api/routines/:rid/runs`.

## MCP

Có hai chiều.

**Open Design làm MCP server** cho coding agent của bạn (Claude Code, Codex, Cursor): agent đọc và sửa project, gọi lint, export… qua daemon đang chạy.

```bash
od mcp install claude --print      # xem trước lệnh hoặc config sẽ ghi
od mcp install claude              # đăng ký server "open-design" vào Claude Code
od mcp install cursor              # ghi vào ~/.cursor/mcp.json, giữ các server khác
od mcp install claude --uninstall  # gỡ khỏi Claude Code
```

`od mcp` (không tham số) là chính server stdio đó; `od mcp live-artifacts` là server thứ hai cho live artifact.

**Open Design làm MCP client**: Settings ▸ MCP servers khai báo server stdio hoặc HTTP của bạn (kèm env, header, OAuth nếu server yêu cầu). Daemon kiểm tra kết nối, liệt kê tool, và khi spawn agent thì ghi các server đang bật vào config của chính agent đó. Secret lưu trong file `0600` dưới thư mục dữ liệu, API chỉ trả giá trị đã che.

## Biến môi trường

| Biến | Ý nghĩa | Mặc định |
|---|---|---|
| `OD_DATA_DIR` | thư mục dữ liệu (SQLite, project, run) | xem [Chạy daemon](#chạy-daemon-và-ui) |
| `OD_PORT`, `OD_HOST` | địa chỉ daemon | `7456`, `127.0.0.1` |
| `OD_API_TOKEN` | token bắt buộc khi bind ra ngoài loopback | — |
| `OD_ALLOWED_ORIGINS` | origin được phép gọi API, cách nhau bởi dấu phẩy | chỉ origin của daemon |
| `OD_DAEMON_URL` | daemon mà các lệnh client gọi tới | `http://127.0.0.1:7456` |
| `OD_RESOURCE_ROOT` | thư mục `content/` (skill, template, design system) | cạnh binary, rồi source tree |
| `OD_UI_DIST` | thư mục UI đã build | `ui/` cạnh binary, rồi `crates/od-ui/dist` |
| `OD_CHROME_PATH` | Chrome/Edge/Chromium cho export | tự tìm |
| `OD_FFMPEG_PATH` | `ffmpeg` cho MP4 | tìm trên `PATH`, Homebrew |
| `OD_TERMINAL_SHELL` | shell của tab terminal | `$SHELL` |
| `OD_MEDIA_CONCURRENCY` | số task media chạy song song | `2` |
| `OD_OPENAI_API_KEY`, `OD_FAL_KEY`, `OD_ELEVENLABS_API_KEY` | key cho tạo ảnh, video, âm thanh (hoặc `media-config.json`) | — |
| `OD_CRITIQUE_ENABLED` | bật Design Jury mặc định | tắt |
| `OD_WATCHER_USE_POLLING` | theo dõi file bằng polling (ổ mạng, Docker) | tắt |
| `OD_OPEN_COMMAND` | lệnh mở thư mục/link thay cho Finder, `xdg-open` | theo hệ điều hành |

## Cấu trúc crate

| Crate | Việc |
|---|---|
| `od-contracts` | DTO serde dùng chung cho daemon, CLI, UI |
| `od-store` | SQLite (WAL, migration theo `user_version`), thư mục project, file version, path an toàn |
| `od-content` | index skill, design template, design system, craft, plugin; ghép prompt legacy |
| `od-runtime` | định nghĩa agent (dữ liệu), phát hiện, spawn, parser stream, run engine, BYOK |
| `od-runs` | run registry: ring event, `events.jsonl`, `state.json`, fan-out, replay |
| `od-preview` | phục vụ preview: chèn bridge, CSP, watcher file, lint artifact, ghi ngược sửa từ preview |
| `od-export` | export không cần trình duyệt: HTML tự chứa, ZIP, ghép PDF/PPTX, trait `Renderer` |
| `od-render-cdp` | `Renderer` qua Chrome DevTools Protocol với Chrome có sẵn; `ffmpeg` cho MP4 |
| `od-media` | tạo ảnh, video, âm thanh bằng key của người dùng (OpenAI Images, fal.ai, ElevenLabs) |
| `od-live` | live artifact: render `html_template_v1`, lưu dưới `.live-artifacts`, làm mới thủ công |
| `od-mcp` | MCP server stdio nối tới daemon, `od mcp install` |
| `od-mcp-client` | MCP server ngoài cho agent: config, client stdio/HTTP, OAuth 2.1 PKCE |
| `od-term` | tab terminal: PTY trong thư mục project, phát lại scrollback, dừng cả cây tiến trình |
| `od-automation` | routine: lịch cron cục bộ, lịch sử, không chạy chồng |
| `od-job` | Windows Job Object để dừng cả cây tiến trình |
| `od-server` | HTTP API (axum), middleware origin/token, SSE, WebSocket |
| `od-cli` | binary `od`: daemon và các lệnh client |
| `od-ui` | UI Leptos CSR biên dịch ra WASM bằng `trunk`, kèm xterm.js trong `vendor/` |
| `apps/desktop` (`od-desktop`) | app Tauri 2 chạy `od-server` trong process |

Nội dung (`content/`) và bridge preview (`bridges/od-bridge.js`) nằm ngoài crate; nguồn gốc và license ghi trong [`NOTICE`](NOTICE).

## Kiểm tra và benchmark

```bash
make check                       # fmt --check + clippy -D warnings + test toàn workspace
crates/od-ui/e2e/run.sh          # e2e Playwright: daemon thật + agent giả, chặn mọi request ra ngoài loopback
```

Máy ít RAM thì giới hạn số job: `CARGO_BUILD_JOBS=2 make check`. `make` đặt `CARGO_TARGET_DIR` riêng cho mỗi worktree (`~/.cache/odrs-target-<tên thư mục>`). Lần đầu e2e cài Playwright vào `crates/od-ui/e2e/node_modules`, cần Chromium của Playwright (`npx playwright install chromium` trong thư mục đó).

Benchmark (criterion):

```bash
cargo bench -p od-content --bench index           # index toàn bộ content/
cargo bench -p od-runs --bench replay             # replay event của run
cargo bench -p od-server --bench sse_latency      # độ trễ stdout của agent → SSE
cargo bench -p od-export --bench standalone       # export HTML tự chứa
```

## Giới hạn đã biết

- **Windows chưa chạy thật.** Job Object, ConPTY và app Tauri trên WebView2 mới qua compile-check.
- **MCP server ngoài chỉ nối với Claude, ACP và OpenCode**, như app gốc. ACP chỉ nhận server stdio.
- **Tiến trình tách hẳn khỏi cây có thể sống sót.** Helper của MCP stdio dùng double-fork, hay job trong terminal vừa `setsid` vừa mất tiến trình cha trong vòng 2 giây, sẽ không bị giết khi đóng tab hoặc tắt server. macOS không có subreaper để chặn hẳn.
- **Sửa text trực tiếp chỉ áp cho phần tử chỉ chứa chữ.** Phần tử có thẻ con hoặc do script tạo ra thì trả 409 kèm lý do.
- **Không có auto-update.** App build từ source: `git pull` rồi build lại.

## License

Apache-2.0, như app gốc. Nội dung lấy từ OpenDesign và thư viện bên thứ ba (xterm.js, MIT) giữ license riêng; chi tiết trong [`NOTICE`](NOTICE).
