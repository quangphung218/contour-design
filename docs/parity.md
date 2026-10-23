# Parity Nét (open-design-rs) so với OpenDesign — đo lại tại waveB-integrate

- Bản Rust: branch `waveB-integrate` HEAD `dc7938a` (2026-10-23 20:46), gồm B1-library, B2-manage, B3-chat, B4-files.
- Mốc so sánh ("Trước"): `docs/parity.md` bản `8c9df6a` (40,7%, main `7681914`). Cùng danh mục, cùng cách chấm, cùng thứ tự dòng.
- App gốc: `~/Documents/GitHub/open-design` (chỉ đọc).
- Cách chấm: **100** đủ, **50** một phần (ghi thiếu gì), **0** không có, **N/A** bỏ có chủ đích. % nhóm là trung bình điểm, không tính N/A. Mỗi dòng nặng như nhau. Chỉ có ở API/CLI, không có trên UI: 50.
- Thay đổi danh mục duy nhất: dòng "Connectors (Composio)" chuyển sang **N/A** theo quyết định đã chốt (cloud). Vì vậy "Sau" chấm 214 dòng, "Trước" chấm 215 dòng. Auto-update vốn đã N/A.
- Chạy thật: daemon `od` debug (`~/.cache/odrs-target-waveB-integrate/debug/od`, build 20:56 sau HEAD) + bundle `crates/od-ui/dist` + mock agent, data dir tạm do từng kịch bản tự tạo và xoá. Chạy 6 kịch bản Playwright của repo từ bản copy ngoài worktree, tuần tự: studio, design-system, library, manage, chat, files. **6/6 PASS**, `externalRequests: 0`. 93 ảnh (light và dark) ở `(audit run) shots/`, log từng kịch bản ở `(audit run) shots/*.log`.
- Bằng chứng dòng nào ghi `e2e <kịch bản> bước N` là đã chạy thật trong lần đo này; còn lại là đọc code (file:dòng tại `dc7938a`).

## Tổng hợp

| Nhóm | Số mục chấm | Đủ | Một phần | Không có | N/A | Trước (40,7%) | Sau | Tăng |
|---|---|---|---|---|---|---|---|---|
| Projects | 14 | 14 | 0 | 0 | 1 | 32 | 100 | +68 |
| Design systems | 13 | 13 | 0 | 0 | 1 | 19 | 100 | +81 |
| Templates/Examples | 6 | 6 | 0 | 0 | 1 | 50 | 100 | +50 |
| MCP | 7 | 7 | 0 | 0 | 1 | 44 | 100 | +56 |
| Phím tắt | 17 | 17 | 0 | 0 | 0 | 44 | 100 | +56 |
| Studio-preview | 26 | 23 | 3 | 0 | 1 | 60 | 94 | +35 |
| Studio-files/tabs | 16 | 15 | 0 | 1 | 0 | 25 | 94 | +69 |
| Export | 11 | 10 | 0 | 1 | 1 | 45 | 91 | +45 |
| Studio-chat | 32 | 27 | 2 | 3 | 1 | 44 | 88 | +44 |
| Plugins/Skills | 8 | 7 | 0 | 1 | 1 | 12 | 88 | +75 |
| Agent runtimes | 8 | 7 | 0 | 1 | 1 | 69 | 88 | +19 |
| Settings | 15 | 12 | 2 | 1 | 1 | 37 | 87 | +50 |
| Desktop shell | 9 | 7 | 1 | 1 | 1 | 56 | 83 | +28 |
| Automations | 8 | 6 | 1 | 1 | 0 | 38 | 81 | +44 |
| Home | 18 | 13 | 3 | 2 | 2 | 39 | 81 | +42 |
| Media | 6 | 4 | 1 | 1 | 0 | 17 | 75 | +58 |
| **Tổng** | **214** | **188** | **13** | **13** | **13** | **40,7** | **90,9** | **+50,2** |

Đọc nhanh:
- **Tăng từ 40,7% lên 90,9%.** 5 nhóm đạt 100%: Projects, Design systems, Templates, MCP, Phím tắt.
- **Còn 26 dòng dưới 100** (13 dòng 50, 13 dòng 0). Phần lớn là tính năng cần mạng ngoài (Figma, marketplace, Web Clipper), nội dung lớn (17 locale, 10 adapter CLI, 13 provider media) hoặc tính năng phụ (pet, browser tab).
- **Khoảng trống đáng làm nhất còn trong Studio-preview**: Manual edit (layers, attributes, đổi ảnh, xoá phần tử), tab Comments, tô màu code.
- `docs/parity.md` trong branch đang lệch: vẫn là bản sau B1 (51,2%). Bản cập nhật của B3 (`2917098`) bị mất khi merge `9759ad9`, B2 và B4 không cập nhật. Dòng tổng của bản đó còn đếm Design systems 12 thay vì 13.

## Dòng còn dưới 100, xếp theo giá trị

Xếp theo giá trị cho người dùng chia công sức. Giờ là ước lượng cho một người đã quen codebase, chưa gồm review.

| # | Dòng | Điểm | Thiếu gì | Giờ | Ghi chú |
|---|---|---|---|---|---|
| 1 | Studio-preview: Manual edit | 50 | Layers, tab Attributes/Html, đổi ảnh, xoá phần tử | 8–12 | Luồng sửa tay chính của app gốc (`ManualEditPanel.tsx`) |
| 2 | Studio-preview: Tab Comments | 50 | Danh sách comment, trạng thái anchor cũ/mất, ảnh trong comment | 4–6 | |
| 3 | Studio-preview: Toggle Preview/Code | 50 | Tô màu cú pháp (CodeView chỉ có số dòng) | 2–3 | |
| 4 | Studio-chat: Import từ Library | 0 | Nguồn Library trong "+" / @ của composer | 2–3 | API library đã có (B1) |
| 5 | Home: Onboarding | 50 | Bước trích brand/design system | 3–4 | Luồng tạo DS từ website đã có, chỉ cần nối vào onboarding |
| 6 | Settings: Memory toàn cục | 50 | Memory hooks, import, sửa MEMORY.md, công tắc bật/tắt | 4–6 | |
| 7 | Studio-chat: Design Jury | 50 | Nút Interrupt, Replay | 3–4 | |
| 8 | Studio-chat: Dòng sinh media | 50 | Dòng tiến trình sinh media trong chat | 3–4 | Daemon chưa phát sự kiện |
| 9 | Home: Recommended for you | 0 | Thẻ gợi ý bắt đầu | 2–3 | |
| 10 | Settings: Menu cài đặt nhanh | 0 | Avatar menu | 1–2 | |
| 11 | Home: Tab entry | 50 | Tab trên Home (rail đã có đủ trang) | 2–3 | Giá trị thấp |
| 12 | Desktop: Menu Help | 50 | Documentation, Report Issue | 1 | Cần chốt link ngoài |
| 13 | Export: JSX | 0 | Export as JSX | 3–4 | |
| 14 | Media: Provider media | 50 | 13 provider còn lại | 2–3 mỗi provider | |
| 15 | Agent runtimes: CLI khác | 0 | 10 adapter (Aider, Amp, Pi, Qoder…) | 2–4 mỗi adapter | |
| 16 | Settings: Ngôn ngữ | 50 | 17 locale | lớn, chủ yếu dịch | |
| 17 | Automations: Crystallize | 0 | Đề xuất skill/memory từ run | 8–12 | |
| 18 | Studio-chat: GenUI surface | 0 | Authorize, Accept all… | 8–12 | |
| 19 | Automations: Orbit | 50 | Orbit settings, daily summary | ? | Phụ thuộc connector, đã N/A. Cần chốt có N/A theo không |
| 20 | Studio-files: Tab browser | 0 | Browser tích hợp | lớn | Cần Tauri webview |
| 21 | Home: From Figma; Studio-chat: Import Figma | 50 / 0 | Figma | ? | Cần mạng ngoài, cần chốt |
| 22 | Plugins: Registry/marketplace | 0 | Registry, trust, marketplace | ? | Cần mạng ngoài, cần chốt |
| 23 | Media: Web Clipper | 0 | Extension trình duyệt | lớn | App gốc đang tắt UI Library |
| 24 | Home: Pet; Desktop: cửa sổ pet | 0 / 0 | Pet | ? | Spec ghi "cân nhắc BỎ", chưa chốt |

## Dòng mới (app gốc có, danh mục cũ thiếu; không tính vào %)

| Nhóm | Tính năng (app gốc) | Điểm | Bản Rust |
|---|---|---|---|
| Studio-preview | Gợi ý một lần khi có artifact đầu tiên (`FirstArtifactHint.tsx`) | 0 | Không có |
| Design systems | Banner thiếu file font thương hiệu (`MissingBrandFontsBanner.tsx`) | 0 | Không có; `od-content/src/design_kit.rs:317` chỉ thay font |
| Studio-chat | Toast khi lưu memory (`MemoryToast.tsx`) | 0 | Không có |
| Agent runtimes | Dialog cài DeepSeek Harness (`DeepSeekHarnessSetupDialog.tsx`) | 50 | Runtime có (`crates/od-runtime/src/parse/dsh.rs`), không có dialog cài |
| Settings | Đăng nhập OAuth xAI (`XaiOAuthControl.tsx`) | 0 | Chỉ có preset xAI dùng key (`crates/od-ui/src/byok.rs:135`) |
| Studio | Gợi ý khôi phục workspace project (`ProjectWorkspaceRecoveryTip.tsx`) | 0 | Không có |

## Bảng đầy đủ

| # | Nhóm | Tính năng (app gốc) | Trước | Sau | Bằng chứng (HEAD dc7938a) / thiếu gì |
|---|---|---|---|---|---|
| 1 | Home | Ô brief, Enter tạo project và gửi lượt đầu | 100 | 100 | `crates/od-ui/src/app/home.rs:399` (nút tạo), Enter gửi lượt đầu; e2e chat bước 6, studio bước 2 |
| 2 | Home | Chip loại đầu ra (Prototype, Website clone, Wireframe, Mobile app, Slide deck, Document, Image, Video, Audio, Motion, HyperFrames, WebGL, Live artifact) | 50 | 100 | 13 chip `crates/od-ui/src/intent.rs:13`; `crates/od-ui/src/app/home.rs:500,518`; e2e chat bước 3; `(audit run) shots/b3-home-light.png` |
| 3 | Home | Chip hành động (Create design system, Create plugin, From Figma, From folder, From template) | 50 | 50 | Có New design system, template, Create plugin (`crates/od-ui/src/app/home.rs:423`), From folder (`home.rs:828`); thiếu From Figma |
| 4 | Home | Chọn design system lúc tạo | 100 | 100 | `crates/od-ui/src/app/home.rs:340` |
| 5 | Home | Chọn template + gallery "Start from a template" | 100 | 100 | `crates/od-ui/src/app/home.rs:369,953`; e2e studio bước 2b |
| 6 | Home | Tiêu đề xoay chữ, carousel placeholder, ví dụ prompt | 0 | 100 | `crates/od-ui/src/app/home.rs:297` tiêu đề xoay, `intent.rs:307` kịch bản placeholder, ví dụ prompt `home.rs:659`; e2e chat bước 2 |
| 7 | Home | Menu "+" thêm ngữ cảnh (đính file, plugin, connector, MCP) | 0 | 100 | `crates/od-ui/src/app/composer.rs:965` PlusMenu (đính file, plugin, skill, MCP, thư mục); connector N/A; e2e chat bước 4–6 |
| 8 | Home | Tìm ngữ cảnh bằng @ (file, plugin, skill, MCP) | 0 | 100 | `crates/od-ui/src/app/composer.rs:681` palette @, `crates/od-ui/src/mention.rs`; e2e chat bước 5 |
| 9 | Home | Picker media ở chân composer (Ratio, Duration, Resolution, Notes) | 0 | 100 | `crates/od-ui/src/app/home.rs:587` MediaBar (model, ratio, resolution, duration, notes `home.rs:625`); e2e chat bước 3 |
| 10 | Home | Working directory picker (chọn, đổi, thư mục gần đây) | 0 | 100 | `crates/od-ui/src/app/home.rs:443,758-795` chọn/đổi/gần đây/bỏ; e2e chat bước 4 |
| 11 | Home | Thẻ "Recommended for you" | 0 | 0 | Không có (app gốc `RecommendedStartRegion.tsx`) |
| 12 | Home | Dải project gần đây (+ lọc owner/type) | 50 | 100 | `crates/od-ui/src/app/projects.rs:684-688` dải gần đây + lọc loại; lọc owner không áp dụng vì không có team (N/A); e2e manage bước 6; `(audit run) shots/manage/shots/home-recent-light.png` |
| 13 | Home | Rail trái (Home, Projects, Automations, Plugins, Design systems, Integrations, thu gọn) | 50 | 100 | `crates/od-ui/src/app/mod.rs:392-404`: Home, Projects, Templates, Design systems, Plugins, Media, Automations, Integrations, Inbox, Settings + thu gọn |
| 14 | Home | Tab entry (Designs, Templates, Design systems, Connectors, Image/Video templates) | 50 | 50 | Home chỉ có gallery template; các tab Design systems/Templates/Connectors nằm ở rail, không phải tab trên Home |
| 15 | Home | Message center (thông báo trong app) | 0 | 100 | `crates/od-ui/src/app/inbox.rs:109` Message center bản local (run xong/lỗi/huỷ, đã đọc, archive); thông báo từ platform ở xa không áp dụng; e2e manage bước 6 |
| 16 | Home | Onboarding (chọn agent, BYOK, test kết nối, trích brand) | 50 | 50 | `crates/od-ui/src/app/onboarding.rs:94-129`: agent + test, BYOK có test và lấy model; thiếu bước trích brand/design system |
| 17 | Home | Đổi theme, đổi ngôn ngữ | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:381` ngôn ngữ, theme ở Settings ▸ Appearance |
| 18 | Home | Pet nổi | 0 | 0 | Không có pet |
| 19 | Home | GitHub Star, Discord, Get help, feature request | N/A | N/A | BỎ (badge, community) |
| 20 | Home | What's new, privacy consent, khảo sát NPS, cloud sign-in, allowance, campaign | N/A | N/A | BỎ (update/telemetry/cloud/billing) |
| 21 | Projects | Lưới project có ảnh bìa | 100 | 100 | `crates/od-ui/src/app/projects.rs:534,808`; `(audit run) shots/manage/shots/projects-grid-light.png` |
| 22 | Projects | Đổi tên, nhân bản, xoá có xác nhận | 100 | 100 | `crates/od-ui/src/app/project_actions.rs:130-145` menu project (đổi tên, nhân bản), xoá có xác nhận |
| 23 | Projects | Badge trạng thái run (Running, Needs input, Completed…) | 0 | 100 | `crates/od-ui/src/app/projects.rs:638` StatusBadge (Running, Needs input, Completed…); e2e manage bước 2 |
| 24 | Projects | Tag loại project | 50 | 100 | `crates/od-ui/src/app/manage.rs:114` 6 tag (Live artifact, Web clone, Slide, Media, Prototype, Other), `projects.rs:851` |
| 25 | Projects | Board (kanban) theo trạng thái | 0 | 100 | `crates/od-ui/src/app/projects.rs:988-1024` board theo trạng thái; `(audit run) shots/manage/shots/projects-board-light.png` |
| 26 | Projects | Chọn nhiều để xoá | 0 | 100 | `crates/od-ui/src/app/projects.rs:464-495` chọn nhiều, xoá có xác nhận; e2e manage |
| 27 | Projects | Lọc, sắp xếp, list view | 0 | 100 | `crates/od-ui/src/app/projects.rs:433` lọc loại, `:444` sắp xếp, `:529` list view; `(audit run) shots/manage/shots/projects-list-light.png` |
| 28 | Projects | Tìm project và conversation (Cmd/Ctrl+K) | 0 | 100 | Cmd/Ctrl+K `crates/od-ui/src/app/mod.rs:200` → `palette.rs:1` (project, trang, file); tìm cả conversation/tin nhắn `projects.rs:424,576` |
| 29 | Projects | Panel tạo project (fidelity, nền tảng đích, speaker notes, animation, media options) | 50 | 100 | `crates/od-ui/src/app/new_project.rs:132-321`: loại, nền tảng, fidelity, speaker notes, animation, media; `(audit run) shots/manage/shots/new-project-panel-light.png` |
| 30 | Projects | Mở folder làm project | 100 | 100 | `crates/od-ui/src/app/projects.rs:366,388-400`; Home `home.rs:828` |
| 31 | Projects | Import ZIP (Claude Design) | 0 | 100 | `crates/od-ui/src/app/projects.rs:345-351` import ZIP Claude Design; e2e manage |
| 32 | Projects | Project instructions | 50 | 100 | `crates/od-ui/src/app/projects.rs:892-965` dialog instructions; `(audit run) shots/manage/shots/project-instructions-light.png` |
| 33 | Projects | Live artifact badge trên thẻ | 0 | 100 | `crates/od-ui/src/app/projects.rs:649-658` LiveBadges |
| 34 | Projects | Recently viewed | 0 | 100 | `crates/od-ui/src/app/projects.rs:414` Recently viewed, ghi nhận ở `mod.rs` ViewTracker |
| 35 | Projects | Team space, invite, presence, sync thiết bị | N/A | N/A | Team/cloud: BỎ |
| 36 | Studio-chat | Stream lượt assistant: text, thinking, tool row, Markdown | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:552`; e2e studio bước 3 |
| 37 | Studio-chat | Stop | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:296` |
| 38 | Studio-chat | Danh sách conversation: tạo, đổi tên, xoá | 50 | 100 | `crates/od-ui/src/app/chat_ops.rs:100-167` đổi tên, xoá có xác nhận; `chat_view.rs:360` New; e2e chat bước 15 |
| 39 | Studio-chat | Fork từ một tin | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:576` |
| 40 | Studio-chat | Session mode (Ask/Plan/Design) | 100 | 100 | `crates/od-ui/src/app/agent_w3.rs:141` |
| 41 | Studio-chat | Đính file, upload trong composer | 0 | 100 | `crates/od-ui/src/app/composer.rs:742-851`, `:978` nút đính, paste, kéo thả; e2e chat bước 7 |
| 42 | Studio-chat | Tham chiếu project khác, link local code | 0 | 100 | `crates/od-ui/src/app/chat_ops.rs:470-611` Link local code, Reference project; e2e chat bước 17 |
| 43 | Studio-chat | Import Figma (.fig, URL) | 0 | 0 | Không có (cần Figma API, mạng ngoài) |
| 44 | Studio-chat | Import từ Library | 0 | 0 | Composer không có nguồn Library (Media library chỉ có chiều ngược: Chat to design `media_page.rs:366`) |
| 45 | Studio-chat | Đổi design system ngay trong composer | 0 | 100 | `crates/od-ui/src/app/chat_ops.rs:638`; e2e chat bước 11 |
| 46 | Studio-chat | Palette @ mention | 0 | 100 | `crates/od-ui/src/app/composer.rs:658-681`; e2e chat bước 8 |
| 47 | Studio-chat | Slash command (/mcp, /search) | 0 | 100 | `crates/od-ui/src/mention.rs` /mcp, /search; e2e chat bước 9 |
| 48 | Studio-chat | Inline model switcher (CLI/BYOK, agent, model) | 0 | 100 | `crates/od-ui/src/app/composer.rs:1115-1196` agent, model, reasoning; e2e chat bước 10 |
| 49 | Studio-chat | Queue: sửa, kéo thả, steer | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:419-471` |
| 50 | Studio-chat | Quote to chat | 0 | 100 | `crates/od-ui/src/app/chat_view.rs:229` quote-selection; e2e chat bước 13c |
| 51 | Studio-chat | Trạng thái run (Working, Thinking) | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:615` |
| 52 | Studio-chat | Todo list + plan pill "bước N/M" | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:396` |
| 53 | Studio-chat | Dòng sinh media, audio player | 0 | 50 | Audio player trong turn files `crates/od-ui/src/app/chat_ops.rs:325`; không có dòng tiến trình sinh media |
| 54 | Studio-chat | "Files from this turn" (Open/Download) | 50 | 100 | `crates/od-ui/src/app/chat_ops.rs:285-333` Open, Download, +N; e2e chat bước 13 |
| 55 | Studio-chat | Copy tin nhắn | 0 | 100 | `crates/od-ui/src/app/chat_ops.rs:349` |
| 56 | Studio-chat | "Continue remaining tasks" | 0 | 100 | `crates/od-ui/src/app/chat_ops.rs:379-381`; e2e chat bước 12 |
| 57 | Studio-chat | Question form | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:833-848` |
| 58 | Studio-chat | GenUI surface (Authorize, Accept all…) | 0 | 0 | Không có GenUI surface |
| 59 | Studio-chat | Next-step actions, suggested next steps | 0 | 100 | `crates/od-ui/src/app/chat_view.rs:652` |
| 60 | Studio-chat | Design toolbox follow-up (Match, Icons, Motion, Polish…) | 0 | 100 | `crates/od-ui/src/app/composer.rs:990-1004`, `chat_view.rs:696`; e2e chat bước 13b |
| 61 | Studio-chat | Design Jury (critique theater) | 50 | 50 | `crates/od-ui/src/app/agent_w3.rs:189,482-519`: bật, vòng, vai, điểm, trạng thái interrupted; thiếu nút Interrupt và Replay |
| 62 | Studio-chat | Lỗi run, reconnect, retry | 50 | 100 | `crates/od-ui/src/app/chat_ops.rs:405` Reconnect, Retry `chat_view.rs:183`; e2e chat bước 14 |
| 63 | Studio-chat | Xuất log/conversation | 100 | 100 | `crates/od-ui/src/app/agent_w3.rs:354-366` |
| 64 | Studio-chat | Memory của project (+ "Remember that…") | 100 | 100 | `crates/od-ui/src/app/agent_w3.rs:238-309` |
| 65 | Studio-chat | Side chat | 100 | 100 | `crates/od-ui/src/app/agent_w3.rs:749-791` |
| 66 | Studio-chat | Thu gọn/resize pane chat | 0 | 100 | `crates/od-ui/src/app/chat_view.rs:373` thu gọn, `chat_ops.rs:214` resize, `studio.rs:1099` Show chat; e2e chat bước 16 |
| 67 | Studio-chat | Cảnh báo deliverable sau run | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:1324` |
| 68 | Studio-chat | Thumbs feedback, share to community, support dialog, balance gate | N/A | N/A | BỎ (telemetry/share/community/billing) |
| 69 | Studio-preview | Preview HTML sandbox + live reload | 100 | 100 | `crates/od-ui/src/app/studio.rs:1296-1309`; e2e studio bước 4–5 (reload 47 ms) |
| 70 | Studio-preview | Toggle Preview / Code | 0 | 50 | Toggle `crates/od-ui/src/app/studio.rs:1325-1335`, CodeView `:1520`: có số dòng, không tô màu cú pháp |
| 71 | Studio-preview | Chọn thiết bị Desktop/Tablet/Mobile | 100 | 100 | `crates/od-ui/src/app/studio.rs:46-55` Desktop/Tablet/Phone |
| 72 | Studio-preview | Zoom in/out/reset | 0 | 100 | `crates/od-ui/src/app/studio.rs:1366-1372`, phím Cmd +/−/0 |
| 73 | Studio-preview | Reload | 100 | 100 | `crates/od-ui/src/app/studio.rs:289` reload_preview |
| 74 | Studio-preview | Chụp màn hình vào clipboard | 0 | 100 | `crates/od-ui/src/app/capture.rs:140`; e2e files bước 10 |
| 75 | Studio-preview | Screenshot to chat | 50 | 100 | `crates/od-ui/src/app/capture.rs:143`; e2e files bước 10 |
| 76 | Studio-preview | Comment mode: ghim, gửi kèm lượt sau | 100 | 100 | `crates/od-ui/src/app/preview_comments.rs:107-193` |
| 77 | Studio-preview | Tab Comments, board, trạng thái anchor cũ/mất, đính ảnh vào comment | 50 | 50 | Chip comment trên composer; thiếu tab danh sách comment, trạng thái anchor cũ/mất, ảnh trong comment |
| 78 | Studio-preview | Draw/Mark (pen, box, text, undo/redo) | 50 | 100 | `crates/od-ui/src/app/preview_tools.rs:1228-1251` pen, box, text, undo, redo, clear; e2e files bước 13 |
| 79 | Studio-preview | Manual edit (layers, Content/Style/Attributes/Html, style controls, upload ảnh, xoá phần tử) | 50 | 50 | Inspect, CSS override, edit text, Save to source (`crates/od-ui/src/app/preview_tools.rs:764-782`); thiếu layers, Attributes/Html, đổi ảnh, xoá phần tử |
| 80 | Studio-preview | Inspect panel | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:663` |
| 81 | Studio-preview | Tweaks | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:857` |
| 82 | Studio-preview | Palette swap 5 preset | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:615` |
| 83 | Studio-preview | Lint anti-slop | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:580,888-915` |
| 84 | Studio-preview | Lịch sử phiên bản: xem, khôi phục | 100 | 100 | `crates/od-ui/src/app/versions.rs:92-149` |
| 85 | Studio-preview | Deck: điều hướng, rail thumbnail | 100 | 100 | `crates/od-ui/src/app/deck.rs:51-278` |
| 86 | Studio-preview | Speaker notes: xem, sửa, lưu | 50 | 100 | `crates/od-ui/src/app/deck.rs:357-397` sửa và lưu vào deck; e2e files bước 11 |
| 87 | Studio-preview | Present: trong tab / fullscreen / tab mới / presenter mode | 50 | 100 | `crates/od-ui/src/app/deck.rs:167-177` trong tab, fullscreen, tab mới, presenter (`presenter.rs:245-282`, đồng bộ); `(audit run) shots/b4/presenter-light.png` |
| 88 | Studio-preview | Markdown viewer/editor (Editor/Split/Preview) | 0 | 100 | `crates/od-ui/src/app/doc_view.rs:25-271` Editor/Split/Preview, auto-save; `(audit run) shots/b4/markdown-split-light.png` |
| 89 | Studio-preview | Live artifact viewer (Preview/Code/Data/Refresh history) | 0 | 100 | `crates/od-ui/src/app/live_view.rs:186-232` Preview/Code/Data/Refresh/History; `(audit run) shots/b4/live-artifact-light.png` |
| 90 | Studio-preview | Viewer ảnh | 100 | 100 | `crates/od-ui/src/app/studio.rs:1279` |
| 91 | Studio-preview | Viewer PDF/Office/video/audio | 50 | 100 | `crates/od-ui/src/app/doc_view.rs:345-406` PDF, video, audio, Office có trích text, Download/Open |
| 92 | Studio-preview | Finalize design package (DESIGN.md) | 50 | 100 | `crates/od-ui/src/app/export_menu.rs:334,378-409` Finalize/Re-finalize theo DESIGN.md; e2e files bước 12c |
| 93 | Studio-preview | Hand off: chọn editor, framework, copy prompt, Continue in CLI | 50 | 100 | `crates/od-ui/src/app/export_extra.rs:156-194` framework, target editor, prompt; Cmd+Shift+K `files_panel.rs:1393`; `(audit run) shots/b4/handoff-dialog-light.png` |
| 94 | Studio-preview | Brand ready prompt | 0 | 100 | `crates/od-ui/src/app/project_actions.rs:182-209` brand progress/ready/use/dismiss; `(audit run) shots/b4/brand-ready-light.png` |
| 95 | Studio-preview | Share link, visibility | N/A | N/A | Share/cloud: BỎ |
| 96 | Studio-files/tabs | Panel file (cây, breadcrumb) | 50 | 100 | `crates/od-ui/src/app/files_panel.rs:575-681` cây, thư mục thu gọn có đếm, breadcrumb |
| 97 | Studio-files/tabs | Upload, paste as text file, kéo thả | 0 | 100 | `crates/od-ui/src/app/files_panel.rs:481-484,544` upload, paste as text, kéo thả; e2e files bước 2, 4 |
| 98 | Studio-files/tabs | New sketch | 0 | 100 | `crates/od-ui/src/app/files_panel.rs:494`, `sketch.rs:168`; `(audit run) shots/b4/sketch-light.png` |
| 99 | Studio-files/tabs | New document (Markdown từ mẫu) | 0 | 100 | `crates/od-ui/src/app/files_panel.rs:488` |
| 100 | Studio-files/tabs | Thư mục: tạo, chuyển | 0 | 100 | `crates/od-ui/src/app/files_panel.rs:497`, move `:909` |
| 101 | Studio-files/tabs | Menu dòng: open in tab, download, copy path, delete | 0 | 100 | `crates/od-ui/src/app/files_panel.rs:902-910` open, new tab, download, copy path, full path, rename, move, delete |
| 102 | Studio-files/tabs | Chọn nhiều: tải ZIP, xoá | 0 | 100 | `crates/od-ui/src/app/files_panel.rs:803-808` ZIP, move, delete |
| 103 | Studio-files/tabs | Nhóm/lọc theo loại, phân trang | 0 | 100 | `crates/od-ui/src/app/files_panel.rs:527,733-766` theo loại, lọc, show more; `(audit run) shots/b4/files-by-kind-light.png` |
| 104 | Studio-files/tabs | Tạo design system từ project | 0 | 100 | `crates/od-ui/src/app/project_actions.rs:141`; e2e files bước 12d |
| 105 | Studio-files/tabs | Thanh tab file, đóng tab | 100 | 100 | `crates/od-ui/src/app/studio.rs:1123-1146` |
| 106 | Studio-files/tabs | Tab launcher "+" (mở file, tạo mới) | 50 | 100 | `crates/od-ui/src/app/files_panel.rs:1282-1301` mở file, document, page, terminal |
| 107 | Studio-files/tabs | Tab terminal | 100 | 100 | `crates/od-ui/src/app/terminal.rs:294` |
| 108 | Studio-files/tabs | Tab browser tích hợp | 0 | 0 | Không có tab browser |
| 109 | Studio-files/tabs | Pages list, tạo page | 0 | 100 | `crates/od-ui/src/app/files_panel.rs:360,491` nhóm Pages, tạo page |
| 110 | Studio-files/tabs | Focus workspace / Show chat | 0 | 100 | `crates/od-ui/src/app/files_panel.rs:1343`, Show chat `studio.rs:1099`; `(audit run) shots/b4/focus-workspace-light.png` |
| 111 | Studio-files/tabs | Quick switcher Cmd/Ctrl+P | 100 | 100 | `crates/od-ui/src/app/switcher.rs:72` |
| 112 | Export | PDF | 100 | 100 | `crates/od-ui/src/app/export_menu.rs:17-40` Pdf |
| 113 | Export | PPTX ảnh | 100 | 100 | `crates/od-ui/src/app/export_menu.rs` Pptx |
| 114 | Export | PPTX editable | 0 | 100 | `crates/od-ui/src/app/export_menu.rs:46` export-pptx-editable (qua Chrome cục bộ) |
| 115 | Export | ZIP project | 100 | 100 | `crates/od-ui/src/app/export_menu.rs` Zip |
| 116 | Export | HTML standalone | 100 | 100 | `crates/od-ui/src/app/export_menu.rs` Html |
| 117 | Export | Export Markdown (file) | 0 | 100 | `crates/od-ui/src/app/export_menu.rs` Markdown; e2e files bước 12 |
| 118 | Export | Export ảnh có chọn định dạng | 50 | 100 | PNG, JPEG, WebP `crates/od-ui/src/app/export_menu.rs:21-23` |
| 119 | Export | Export JSX / preview as HTML | 0 | 0 | Không có Export as JSX |
| 120 | Export | Save as template | 0 | 100 | `crates/od-ui/src/app/export_menu.rs:320`, `export_extra.rs:74-100` |
| 121 | Export | MP4 HyperFrames | 50 | 100 | MP4 trong menu Export `crates/od-ui/src/app/export_menu.rs:26` + trang Media `media_page.rs:887` |
| 122 | Export | Open in new tab, copy path, download file | 0 | 100 | `crates/od-ui/src/app/export_menu.rs:308-316` |
| 123 | Export | Deploy Vercel/Cloudflare, social share, quick share | N/A | N/A | Deploy/share: BỎ |
| 124 | Design systems | Trang Design systems (Your systems, presets, lọc) | 0 | 100 | `crates/od-ui/src/app/ds_page.rs:177-315`; e2e library PASS |
| 125 | Design systems | Chọn design system khi tạo project | 100 | 100 | `crates/od-ui/src/app/home.rs:340`, `new_project.rs:162` |
| 126 | Design systems | Tạo DS từ mô tả thương hiệu | 100 | 100 | `crates/od-ui/src/app/ds_create.rs:141-187` |
| 127 | Design systems | Tạo DS từ URL, GitHub, file, Figma, paste DESIGN.md, copy DS có sẵn | 0 | 100 | `crates/od-ui/src/app/ds_create.rs:519-584` website, GitHub, Figma qua agent, file, DESIGN.md/CSS, copy bộ có sẵn; e2e library fromWebsite, fromCss |
| 128 | Design systems | "Start from a brand" picker | 0 | 100 | `crates/od-ui/src/app/ds_create.rs:519,628` brand picker |
| 129 | Design systems | Tiến trình sinh theo pha | 50 | 100 | `crates/od-ui/src/app/ds_create.rs:569` 4 pha |
| 130 | Design systems | Review (Looks good / Needs work), revision accept/reject | 0 | 100 | `crates/od-ui/src/app/ds_page.rs:917-947,1047-1068` review, revision nhận/bỏ |
| 131 | Design systems | Kit view (Showcase, Tokens, DESIGN.md), brand detail, preview modal | 0 | 100 | `crates/od-ui/src/app/ds_page.rs:990-1204` showcase, tokens, DESIGN.md, fullscreen |
| 132 | Design systems | Sửa/copy/lưu DESIGN.md, upload font/logo | 0 | 100 | `crates/od-ui/src/app/ds_page.rs:1002-1015,1149` sửa/chép DESIGN.md, upload asset |
| 133 | Design systems | Default for new chats | 0 | 100 | `crates/od-ui/src/app/ds_page.rs:878` |
| 134 | Design systems | Download DS, Edit with agent | 0 | 100 | `crates/od-ui/src/app/ds_page.rs:872,893` |
| 135 | Design systems | Import DS từ GitHub/shadcn trong Settings | 0 | 100 | Trong luồng Tạo của trang Design systems (GitHub qua agent, theme shadcn dán vào); không tải registry shadcn vì không ra mạng |
| 136 | Design systems | Rebuild token contract, điểm chất lượng | 0 | 100 | `crates/od-ui/src/app/ds_page.rs:1135-1143` điểm, rebuild |
| 137 | Design systems | Publish, share/sync to team | N/A | N/A | Publish/team: BỎ |
| 138 | Plugins/Skills | Danh sách, chi tiết plugin | 50 | 100 | `crates/od-ui/src/app/plugins_page.rs:234-332,552` |
| 139 | Plugins/Skills | Dùng plugin (Use, Remix) | 50 | 100 | `crates/od-ui/src/app/plugins_page.rs:607-627`; e2e library pluginUse |
| 140 | Plugins/Skills | Form input plugin | 0 | 100 | `crates/od-ui/src/app/plugins_page.rs:616` InputsForm |
| 141 | Plugins/Skills | Thêm plugin (từ prompt, link, folder), install/uninstall | 0 | 100 | `crates/od-ui/src/app/plugins_page.rs:930-971` agent, link, folder; uninstall `:595` |
| 142 | Plugins/Skills | Registry sources, trust, marketplace | 0 | 0 | Không có registry/marketplace (cần mạng ngoài) |
| 143 | Plugins/Skills | Danh sách, chi tiết skill | 0 | 100 | `crates/od-ui/src/app/plugins_page.rs:251,705` |
| 144 | Plugins/Skills | Sửa skill (tạo, sửa, xoá, user override) | 0 | 100 | `crates/od-ui/src/app/plugins_page.rs:734-836` tạo, sửa (override), xoá |
| 145 | Plugins/Skills | Plugin rail trong composer | 0 | 100 | Plugin vào composer qua menu "+" và @ (`crates/od-ui/src/app/composer.rs:1045`), đúng hành vi app gốc (rail tắt, `showRail={false}`) |
| 146 | Plugins/Skills | Publish/contribute plugin, share with team | N/A | N/A | Publish/community/team: BỎ |
| 147 | Templates/Examples | Gallery có lọc (loại, surface, nhóm ngành) | 50 | 100 | `crates/od-ui/src/app/templates_page.rs:222-246` |
| 148 | Templates/Examples | Thẻ template có preview sống | 100 | 100 | `crates/od-ui/src/app/templates_page.rs:259,379`, `lazy_frame.rs` |
| 149 | Templates/Examples | Dùng template để tạo | 100 | 100 | `crates/od-ui/src/app/templates_page.rs:407`, Home `home.rs:953` |
| 150 | Templates/Examples | Preview modal (fullscreen, Remix, export) | 0 | 100 | `crates/od-ui/src/app/templates_page.rs:399-407` fullscreen, tab mới, tải, Remix, Use |
| 151 | Templates/Examples | Prompt template ảnh/video (Copy prompt) | 50 | 100 | `crates/od-ui/src/app/templates_page.rs:440-488` Copy, Generate |
| 152 | Templates/Examples | Template do user lưu | 0 | 100 | `crates/od-ui/src/app/templates_page.rs:541-601` |
| 153 | Templates/Examples | Community gallery, share template | N/A | N/A | Community/share: BỎ |
| 154 | Settings | Chọn agent + rescan | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:240`; `(audit run) shots/manage/shots/settings-light.png` |
| 155 | Settings | Model, reasoning effort, service tier | 50 | 100 | `crates/od-ui/src/app/settings_page.rs:289,305,316` model, reasoning, service tier |
| 156 | Settings | Advanced: proxy, đường dẫn tuỳ chỉnh | 0 | 100 | `crates/od-ui/src/app/settings_page.rs:442-519` Advanced env (proxy, đường dẫn) |
| 157 | Settings | Test kết nối agent | 0 | 100 | `crates/od-ui/src/app/settings_page.rs:326`; e2e manage bước 3 |
| 158 | Settings | BYOK: preset provider, key, base URL, fetch models | 50 | 100 | `crates/od-ui/src/app/byok_form.rs:216-295` preset, protocol, base URL, key, fetch models, test, save; `(audit run) shots/manage/shots/settings-api-light.png` |
| 159 | Settings | Quyền agent (Automatic / Ask first) | 100 | 100 | Settings ▸ quyền agent (Automatic/Ask first) |
| 160 | Settings | Ngôn ngữ | 50 | 50 | en + vi; app gốc 19 locale |
| 161 | Settings | Theme | 100 | 100 | Settings ▸ Appearance |
| 162 | Settings | Thông báo: âm thanh, desktop notification | 0 | 100 | `crates/od-ui/src/app/notify.rs:236-264` âm thanh, desktop, test |
| 163 | Settings | Project locations | 0 | 100 | `crates/od-ui/src/app/settings_page.rs:727-770` thêm, mặc định, scan |
| 164 | Settings | Global rules / instructions | 0 | 100 | `crates/od-ui/src/app/settings_page.rs:558-570` |
| 165 | Settings | Memory toàn cục (loại, hooks, import, MEMORY.md) | 50 | 50 | `crates/od-ui/src/app/settings_page.rs:618-653` memory toàn cục có loại; thiếu memory hooks, import, sửa MEMORY.md, công tắc bật/tắt |
| 166 | Settings | Media providers (key, base URL) | 50 | 100 | `crates/od-ui/src/app/settings_page.rs:854-895`; `(audit run) shots/manage/shots/settings-media-light.png` |
| 167 | Settings | About (version, export diagnostics cục bộ, reset onboarding) | 0 | 100 | `crates/od-ui/src/app/settings_page.rs:935-952` version, export diagnostics, reset onboarding |
| 168 | Settings | Menu cài đặt nhanh (avatar menu) | 0 | 0 | Không có avatar menu cài đặt nhanh |
| 169 | Settings | Labs, privacy, account, wallet, update | N/A | N/A | BỎ (labs/privacy/account/wallet/update) |
| 170 | Agent runtimes | Claude Code (stream, resume, steer) | 100 | 100 | `crates/od-runtime`; e2e studio (mock claude) |
| 171 | Agent runtimes | Codex | 100 | 100 | `crates/od-runtime` |
| 172 | Agent runtimes | Cursor, OpenCode, Copilot, Qwen | 100 | 100 | `crates/od-runtime` |
| 173 | Agent runtimes | Họ ACP (Kimi, Hermes, Kiro, Kilo, Vibe, Trae, Reasonix, Devin, DeepSeek Harness) | 100 | 100 | `crates/od-runtime/src/parse/dsh.rs` và họ ACP |
| 174 | Agent runtimes | CLI khác (Aider, Amp, Pi, Qoder, MiMo, Codebuddy, AtomCode, Grok Build, Antigravity, DeepSeek TUI) | 0 | 0 | Không có adapter Aider, Amp, Pi, Qoder, MiMo CLI, Codebuddy, AtomCode, Grok Build, Antigravity, DeepSeek TUI |
| 175 | Agent runtimes | Phát hiện agent + Refresh | 100 | 100 | `GET /api/agents`, `crates/od-ui/src/app/onboarding.rs:120` Refresh |
| 176 | Agent runtimes | Test kết nối | 0 | 100 | `crates/od-ui/src/app/settings_page.rs:326`, onboarding test; e2e manage bước 1 |
| 177 | Agent runtimes | BYOK preset (26 provider) | 50 | 100 | `crates/od-ui/src/byok.rs`: 24/26 preset (thiếu Azure OpenAI, vLLM) |
| 178 | Agent runtimes | AMR cloud | N/A | N/A | Cloud: BỎ |
| 179 | MCP | MCP server `od mcp` (stdio) | 100 | 100 | `crates/od-cli/src/mcp_cmd.rs`, `crates/od-mcp`; e2e studio bước 15d |
| 180 | MCP | Cài MCP vào agent (tab theo client, copy command) | 50 | 100 | `crates/od-ui/src/app/integrations.rs:241-302` snippet theo client, deeplink, Install/Uninstall; `(audit run) shots/manage/shots/integrations-agents-light.png` |
| 181 | MCP | MCP server ngoài (stdio/HTTP, args, test) | 100 | 100 | `crates/od-ui/src/app/mcp_settings.rs:78-249` |
| 182 | MCP | OAuth MCP | 100 | 100 | `crates/od-ui/src/app/mcp_settings.rs:246` Connect/Sign in |
| 183 | MCP | Catalog mẫu MCP | 0 | 100 | `crates/od-ui/src/app/integrations.rs:181-208` |
| 184 | MCP | Ghi chú forward tới agent nào | 0 | 100 | `crates/od-ui/src/app/integrations.rs:111-115` |
| 185 | MCP | Connectors (Composio) | 0 | N/A | Connectors Composio: N/A (cloud, quyết định Atlat 23/10) |
| 186 | MCP | Use everywhere (hướng dẫn CLI + MCP) | 0 | 100 | `crates/od-ui/src/app/integrations.rs:333-350`; `(audit run) shots/manage/shots/integrations-everywhere-light.png` |
| 187 | Media | Provider media | 50 | 50 | `crates/od-media/src/provider.rs`: OpenAI, fal, ElevenLabs + base URL tuỳ chỉnh; app gốc 16 provider |
| 188 | Media | Project ảnh/video/audio trong UI (model, aspect, voice) | 0 | 100 | `crates/od-ui/src/app/media_page.rs:615-734` |
| 189 | Media | Audio player trong chat | 0 | 100 | Audio trong chat `crates/od-ui/src/app/chat_ops.rs:325`, thư viện `media_page.rs:437` |
| 190 | Media | Library (lọc, upload, Chat to design) | 0 | 100 | `crates/od-ui/src/app/media_page.rs:346-404` |
| 191 | Media | Web Clipper | 0 | 0 | Không có Web Clipper |
| 192 | Media | HyperFrames → MP4 | 50 | 100 | `crates/od-ui/src/app/media_page.rs:887-909` |
| 193 | Automations | Trang Automations | 100 | 100 | `crates/od-ui/src/app/automations.rs:433` |
| 194 | Automations | Tạo automation (tên, prompt, lịch, project) | 50 | 100 | `crates/od-ui/src/app/automations.rs:446-504,613-677` preset hourly/daily/weekly, timezone, tạo project mới mỗi lần |
| 195 | Automations | Thao tác dòng: Run now, bật/tắt, xoá, mở conversation, sửa | 50 | 100 | `crates/od-ui/src/app/automations.rs:763-774` bật/tắt, Run now, Edit, xoá |
| 196 | Automations | Lịch sử run | 100 | 100 | `crates/od-ui/src/app/automations.rs:791-806` |
| 197 | Automations | Crystallize, evolution proposals | 0 | 0 | Không có Crystallize, evolution proposals |
| 198 | Automations | Template automation | 0 | 100 | `crates/od-ui/src/app/automations.rs:76-120` 7 template của app gốc |
| 199 | Automations | Orbit (tóm tắt connector hằng ngày) | 0 | 50 | Chỉ có template routine `orbit-daily` (`crates/od-ui/src/app/automations.rs:111`); không có Orbit settings/daily summary; phụ thuộc connector |
| 200 | Automations | Refresh live artifact | 0 | 100 | `crates/od-ui/src/app/live_view.rs:199` Refresh + history; template live-artifact maintainer |
| 201 | Desktop shell | Cửa sổ chính, daemon trong process | 100 | 100 | `apps/desktop/src/main.rs:258` |
| 202 | Desktop shell | Menu App/Edit/View/Window | 100 | 100 | `apps/desktop/src/main.rs:196-204` Menu::default + Project |
| 203 | Desktop shell | Menu Help (Documentation, Report Issue) | 0 | 50 | Menu Help có Shortcuts, Command palette, Show Data Folder (`apps/desktop/src/main.rs:30-33,235`); thiếu Documentation, Report Issue |
| 204 | Desktop shell | Folder picker native | 100 | 100 | `apps/desktop/src/main.rs` pick_folder |
| 205 | Desktop shell | Mở thư mục trong Finder | 100 | 100 | Handoff ▸ Open project folder, Help ▸ Show Data Folder |
| 206 | Desktop shell | Export PDF/ảnh bằng engine trình duyệt | 100 | 100 | Chrome cục bộ qua CDP (decisions.md #2) |
| 207 | Desktop shell | Splash | 0 | 100 | `apps/desktop/static/index.html:6` splash khi daemon khởi động |
| 208 | Desktop shell | Đồng bộ theme native | 0 | 100 | `apps/desktop/src/main.rs:121-131` set_theme theo Appearance |
| 209 | Desktop shell | Cửa sổ pet | 0 | 0 | Không có cửa sổ pet |
| 210 | Desktop shell | Auto-update, invite deep link, crash diagnostics, Develop menu AMR | N/A | N/A | auto-update N/A (quyết định); cloud/telemetry BỎ |
| 211 | Phím tắt | Cmd/Ctrl+, mở Settings | 0 | 100 | `crates/od-ui/src/app/mod.rs:212` |
| 212 | Phím tắt | Cmd/Ctrl+K tìm project | 0 | 100 | `crates/od-ui/src/app/mod.rs:200` (palette) |
| 213 | Phím tắt | Cmd/Ctrl+B thu gọn rail | 0 | 100 | `crates/od-ui/src/app/mod.rs:216-221`; trong Studio là ẩn/hiện panel file `studio.rs:821` |
| 214 | Phím tắt | Cmd/Ctrl+P quick switcher | 100 | 100 | `crates/od-ui/src/app/studio.rs:817` |
| 215 | Phím tắt | Cmd/Ctrl+T tab mới | 50 | 100 | `crates/od-ui/src/app/files_panel.rs:1375` mở tab launcher |
| 216 | Phím tắt | Cmd/Ctrl+W đóng tab | 100 | 100 | `crates/od-ui/src/app/studio.rs:837` |
| 217 | Phím tắt | Ctrl+Tab / Ctrl+Shift+Tab | 100 | 100 | `crates/od-ui/src/app/studio.rs:857` |
| 218 | Phím tắt | Ctrl+PageUp/PageDown, Cmd+Opt+←/→ | 0 | 100 | `crates/od-ui/src/app/files_panel.rs:1382-1389` |
| 219 | Phím tắt | Cmd/Ctrl+1…9 | 100 | 100 | `crates/od-ui/src/app/studio.rs:845` |
| 220 | Phím tắt | Cmd/Ctrl+Shift+K Continue in CLI | 0 | 100 | `crates/od-ui/src/app/files_panel.rs:1393` |
| 221 | Phím tắt | Enter / Shift+Enter gửi, xuống dòng | 100 | 100 | `crates/od-ui/src/app/composer.rs`, `chat_view.rs` |
| 222 | Phím tắt | @ và / trong composer | 0 | 100 | `crates/od-ui/src/app/composer.rs:681`, `mention.rs` |
| 223 | Phím tắt | Cmd/Ctrl+Enter gửi comment | 50 | 100 | `crates/od-ui/src/app/preview_comments.rs:179` |
| 224 | Phím tắt | Cmd/Ctrl+Z trong draw | 0 | 100 | `crates/od-ui/src/app/preview_tools.rs:1157` |
| 225 | Phím tắt | Deck: →, ←, PageUp/Down, Home, End, R | 50 | 100 | `crates/od-ui/src/app/studio.rs:879-882`, R ở presenter `presenter.rs:211` |
| 226 | Phím tắt | Esc đóng modal, present, mode | 100 | 100 | `crates/od-ui/src/app/studio.rs:865` |
| 227 | Phím tắt | Phím trong DS kit (E/C/U/R/⌫) | 0 | 100 | `crates/od-ui/src/app/ds_page.rs:661-666` (E/C/U/R/⌫) |
