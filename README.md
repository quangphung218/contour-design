# open-design-rs

Bản viết lại OpenDesign (nexu-io/open-design, Apache-2.0) bằng Rust: app local, không đăng nhập, không cloud.

## Chạy

```bash
cargo run -p od-cli --bin od                # daemon tại http://127.0.0.1:7456
cargo run -p od-cli --bin od -- project create --name "Landing"
cargo run -p od-cli --bin od -- files write <project-id> index.html --content "<h1>Hi</h1>"
cargo run -p od-cli --bin od -- --json project list
cargo run -p od-cli --bin od -- run start --project <project-id> --agent claude --message "Làm landing page" --watch
cargo run -p od-cli --bin od -- run cancel <run-id>
```

`od run watch <run-id> [--after N]` in event của run dạng NDJSON (`{id, event, data}`), thoát 0 khi run `succeeded`.

```bash
cargo run -p od-cli --bin od -- media generate --project <id> --surface image --model gpt-image-1 --prompt "…" [--no-wait]
cargo run -p od-cli --bin od -- media wait|cancel <task-id>      # media tasks [--project], models, templates
cargo run -p od-cli --bin od -- plugin list|info|atoms
cargo run -p od-cli --bin od -- plugin apply <id> -i key=value [--grant-caps mcp,fs:read]   # thiếu capability: exit 66
cargo run -p od-cli --bin od -- plugin validate <thư-mục-plugin>
cargo run -p od-cli --bin od -- export <project-id> deck.html --format pptx-editable
```

Media chạy qua hàng đợi (`OD_MEDIA_CONCURRENCY`, mặc định 2): task `queued` được chạy lại sau restart, task đang `running` thành `interrupted` (không gọi provider lần hai).
Design system mới: Home → "New design system…" (hoặc `POST /api/design-systems/generate`), agent viết `DESIGN.md` + `tokens.css`, package lưu ở `<OD_DATA_DIR>/design-systems/<id>` và hiện là `user:<id>`.
Quyền agent: mặc định tự duyệt; ghi `{"agent_permission_mode": "ask"}` vào `<OD_DATA_DIR>/app-config.json` để siết lại.

Biến môi trường: `OD_DATA_DIR` (dữ liệu), `OD_RESOURCE_ROOT` (thư mục `content/`), `OD_PORT`, `OD_HOST`,
`OD_API_TOKEN` (bắt buộc khi bind ngoài loopback), `OD_ALLOWED_ORIGINS`, `OD_DAEMON_URL` (cho lệnh client).

## Crate

| Crate | Việc |
|---|---|
| `od-contracts` | DTO serde dùng chung |
| `od-store` | SQLite (WAL, migration theo `user_version`), thư mục project, file version, path an toàn |
| `od-content` | Index skill, design template, design system, craft; ghép prompt legacy |
| `od-runs` | Run registry: ring 2.000 event, `events.jsonl`, `state.json`, fan-out, replay |
| `od-runtime` | RuntimeDef (Claude, Codex exec-json), phát hiện, spawn theo process group, parser, engine |
| `od-server` | HTTP API (axum) |
| `od-cli` | Binary `od` |

Check: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.
Bench: `cargo bench -p od-content --bench index`, `cargo bench -p od-runs --bench replay`,
`cargo bench -p od-server --bench sse_latency` (độ trễ stdout→SSE).
