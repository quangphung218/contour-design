# open-design-rs

Bản viết lại OpenDesign (nexu-io/open-design, Apache-2.0) bằng Rust: app local, không đăng nhập, không cloud.

## Chạy

```bash
cargo run -p od-cli --bin od                # daemon tại http://127.0.0.1:7456
cargo run -p od-cli --bin od -- project create --name "Landing"
cargo run -p od-cli --bin od -- files write <project-id> index.html --content "<h1>Hi</h1>"
cargo run -p od-cli --bin od -- --json project list
```

Biến môi trường: `OD_DATA_DIR` (dữ liệu), `OD_RESOURCE_ROOT` (thư mục `content/`), `OD_PORT`, `OD_HOST`,
`OD_API_TOKEN` (bắt buộc khi bind ngoài loopback), `OD_ALLOWED_ORIGINS`, `OD_DAEMON_URL` (cho lệnh client).

## Crate

| Crate | Việc |
|---|---|
| `od-contracts` | DTO serde dùng chung |
| `od-store` | SQLite (WAL, migration theo `user_version`), thư mục project, file version, path an toàn |
| `od-content` | Index skill, design template, design system, craft |
| `od-server` | HTTP API (axum) |
| `od-cli` | Binary `od` |

Check: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.
Bench index content: `cargo bench -p od-content --bench index`.
