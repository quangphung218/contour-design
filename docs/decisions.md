# Quyết định thiết kế

Chốt 2026-10-22 (Atlat, được anh giao toàn quyền). Trả lời 4 câu hỏi ở `spec.md` mục 5.6, cộng các ràng buộc chung.

| # | Câu hỏi | Chốt | Vì sao |
|---|---|---|---|
| 1 | Quyền của agent | Mặc định giống app gốc: agent tự duyệt (`bypassPermissions` cho Claude, cờ tương đương cho CLI khác), cwd = thư mục project. Settings có công tắc `agent_permission_mode = auto | ask` (ask: Claude dùng `acceptEdits`, CLI khác bỏ cờ tự duyệt). | App local một người dùng; chạy không cần trả lời từng lệnh là trải nghiệm chính. Công tắc để người dùng siết lại khi cần. |
| 2 | Chrome cho export | Chỉ dùng Chrome/Edge/Chromium có sẵn trên máy (tìm theo đường dẫn chuẩn + `OD_CHROME_PATH`). Không tự tải Chromium. Không có thì PDF rơi về in của webview, chụp ảnh báo lỗi rõ "cần Chrome". | Không có mạng ra ngoài ngầm; máy anh đã có Chrome. |
| 3 | Design system mang tên thương hiệu | Ship nguyên (app local, không phát hành công khai), giữ attribution và LICENSE con, có `NOTICE`. | Không phân phối nên rủi ro thương hiệu không đáng kể; giữ giá trị nội dung. |
| 4 | Bộ prompt | Chỉ bản legacy (`apps/daemon/src/prompts/` của app gốc), viết lại bằng Rust. Không có rollout/A-B. | Cấu trúc rõ, gọn một chỗ. |

## Ràng buộc chung
- **Không đăng nhập, không cloud, không telemetry, không mạng ra ngoài** trừ gọi agent/provider người dùng cấu hình. Test e2e chặn mọi request ra ngoài loopback.
- **Stack**: Cargo workspace, edition 2024, tokio, axum, rusqlite (bundled), Leptos CSR (UI), Tauri 2 (shell). Layout crate theo `spec.md` 5.2.
- **Hiệu năng**: bám bảng mục tiêu `spec.md` 5.5; mỗi mốc có bench cho phần mình đụng.
- **Code Rust** theo skill `rust-skills` (`.claude/skills/rust-skills`).
- **Máy dev 16 GB**: `.cargo/config.toml` giới hạn `jobs = 4`; test chạy tuần tự khi nặng.
- **Content** (skills, design-templates, design-systems, craft…) copy từ app gốc theo bảng `spec.md` 3.7, giữ mọi LICENSE con; loại prompt-template không rõ license và font không có license.
- **Repo không có remote**: merge vào `main` local sau cổng review của Atlat.
