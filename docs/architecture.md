# Kiến trúc

Tóm tắt một trang. Chi tiết hành vi ở [`spec.md`](spec.md) (mục 5 là đề xuất kiến trúc gốc), quyết định đã chốt ở [`decisions.md`](decisions.md).

## Một API, ba client

```
 trình duyệt ─┐                        ┌─ od-store   (SQLite + thư mục project)
 app desktop ─┼─ HTTP / SSE / WS ─ od-server ─┼─ od-runs    (event của run)
 CLI, MCP ────┘   127.0.0.1                 ├─ od-runtime (spawn agent CLI / BYOK)
                                            ├─ od-preview, od-export, od-render-cdp
                                            └─ od-term, od-automation, od-media, od-mcp-client
```

- **Daemon** (`od-server`, chạy bởi `od` hoặc nhúng trong app desktop) giữ toàn bộ trạng thái; client không giữ gì riêng. Mặc định chỉ bind loopback; bind ra ngoài thì bắt buộc `OD_API_TOKEN`. Middleware chặn origin lạ.
- **UI** (`od-ui`) là Leptos CSR biên dịch ra WASM, được chính daemon phục vụ như file tĩnh. UI chỉ gọi `/api/*`, không có đường tắt nào khác.
- **CLI** `od` và **MCP server** (`od mcp`) là client mỏng của cùng API (luật dual-track: tính năng nào có trong UI cũng gọi được từ CLI/agent).
- **App desktop** (`apps/desktop`, Tauri 2) chạy `od-server` trong process ở port loopback ngẫu nhiên và mở một webview trỏ vào đó. Tauri chỉ lo phần native: hộp thoại chọn thư mục, menu. Capability IPC chỉ cấp cho đúng origin của daemon.

## Luồng một run

1. UI hoặc CLI gửi `POST /api/runs` (project, agent, message).
2. `od-content` ghép prompt legacy: chống prompt injection → chế độ → discovery → charter designer → design system → craft → skill/template → plugin → các override (spec 2.4).
3. `od-runtime` tìm agent trên `PATH`, dựng tham số theo định nghĩa dữ liệu của agent (cờ quyền, resume, model), spawn trong process group với cwd là thư mục project và env đã lọc `OD_*`.
4. Parser theo định dạng stream của agent (Claude stream-json, Codex exec-json, ACP, text) đổi stdout thành `AgentEvent`.
5. `od-runs` ghi event vào ring buffer và `events.jsonl`, phát qua SSE `GET /api/runs/:id/events` (nối lại bằng `Last-Event-ID`).
6. Agent ghi file thẳng vào thư mục project. `od-preview` theo dõi thư mục và báo UI tải lại preview; file HTML có lịch sử version theo nguồn (AI, sửa tay, restore).
7. Hết run: `state.json` ghi trạng thái cuối. UI gọi `GET /api/runs/:id/deliverable` để kiểm run có thật sự tạo file đầu ra; lint artifact chạy khi người dùng, agent hoặc `od lint` yêu cầu.

## Preview

Preview là iframe sandbox. `od-preview` chèn `bridges/od-bridge.js` vào HTML trước mọi script của tác giả; bridge nói chuyện với UI bằng `postMessage` (`od:*`): chọn phần tử để comment, inspect và sửa style/text (ghi ngược vào source), palette, tweaks (artifact khai báo bằng khối `script[type="application/json"][data-od-tweaks]`), deck. Giao thức đầy đủ ở [`bridges/README.md`](../bridges/README.md).

## Dữ liệu

`OD_DATA_DIR` chứa `app.sqlite` (project, conversation, message, file version), `projects/<id>/` (file của project, trừ project mở từ thư mục có sẵn), `runs/<id>/` (`events.jsonl`, `state.json`), `app-config.json` và các file secret `0600` (BYOK, MCP). Một file khoá `daemon.lock` bảo đảm mỗi thư mục dữ liệu chỉ có một daemon.

## Tiến trình con

Agent, tab terminal và MCP server stdio đều chạy trong process group riêng (Windows: Job Object qua `od-job`). Huỷ run, đóng tab hay tắt daemon/app đều dừng cả cây tiến trình: SIGTERM, chờ, rồi SIGKILL.

## Export

`od-export` làm những gì không cần trình duyệt (HTML tự chứa, ZIP, ghép PDF/PPTX từ ảnh). Phần cần render đi qua trait `Renderer`, hiện thực bằng `od-render-cdp` với Chrome/Edge/Chromium có sẵn trên máy; MP4 thêm `ffmpeg`.

## Nội dung

`content/` (skill, design template, design system, craft, plugin, prompt template) được index lúc khởi động (`od-content`), cộng thư mục người dùng dưới `OD_DATA_DIR`. App desktop đóng gói `content/` trong bundle. Nguồn gốc và license ở [`NOTICE`](../NOTICE).
