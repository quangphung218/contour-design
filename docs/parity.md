# Parity open-design-rs so với OpenDesign

- Bản Rust: `open-design-rs` main `7681914` (2026-10-23); cập nhật sau track B1-library (Design systems, Plugins/Skills, Media, Templates/Examples).
- App gốc: `~/Documents/GitHub/open-design` (đọc source, chủ yếu `apps/web/src/i18n/locales/en.ts` và `apps/web/src/components/`).
- Cách chấm: **100** có đủ, **50** một phần (ghi thiếu gì), **0** không có, **N/A** bỏ có chủ đích (spec 1.6, decisions.md). % nhóm là trung bình điểm, không tính N/A. Mỗi mục nặng như nhau, nút nhỏ và luồng lớn cùng một trọng số.
- Tính năng chỉ có ở API hoặc CLI `od`, không có trên UI: chấm 50.
- Chạy thật: daemon `od` debug + bundle `crates/od-ui/dist` + mock agent, data dir tạm, Playwright 1.64.1. 19 ảnh ở `(audit run) shots/` (light, 1440×900).
- App gốc không có tính năng tên "Self-check". Gần nhất là memory hook "Self-verify output" (`apps/web/src/components/MemoryHooksPanel.tsx:8`).

## Tổng hợp

| Nhóm | Số mục | Đủ | Một phần | Không có | N/A | % |
|---|---|---|---|---|---|---|
| Agent runtimes | 8 | 5 | 1 | 2 | 1 | 69 |
| Studio-preview | 26 | 11 | 9 | 6 | 1 | 60 |
| Desktop shell | 9 | 5 | 0 | 4 | 1 | 56 |
| Templates/Examples | 6 | 6 | 0 | 0 | 1 | 100 |
| Export | 11 | 4 | 2 | 5 | 1 | 45 |
| Studio-chat | 32 | 12 | 4 | 16 | 1 | 44 |
| MCP | 8 | 3 | 1 | 4 | 0 | 44 |
| Phím tắt | 17 | 7 | 3 | 7 | 0 | 50 |
| Home | 18 | 4 | 6 | 8 | 2 | 39 |
| Automations | 8 | 2 | 2 | 4 | 0 | 38 |
| Settings | 15 | 3 | 5 | 7 | 1 | 37 |
| Projects | 14 | 3 | 3 | 8 | 1 | 32 |
| Studio-files/tabs | 16 | 3 | 2 | 11 | 0 | 25 |
| Design systems | 12 | 12 | 0 | 0 | 1 | 100 |
| Media | 6 | 3 | 2 | 1 | 0 | 67 |
| Plugins/Skills | 8 | 6 | 0 | 2 | 1 | 75 |
| **Tổng** | **215** | **90** | **40** | **85** | **12** | **51,2** |

Đọc nhanh:
- **Luồng lõi đã chạy tốt**: brief → agent → preview → comment/inspect/edit text → version → export (HTML, ZIP, PDF, PNG, PPTX). Phần preview (60%) và runtime agent (69%) gần app gốc nhất.
- **Màn "thư viện" đã có (track B1-library)**: Design systems, Plugins/Skills, Media (thư viện, sinh, hàng đợi), Templates. Còn thiếu: registry/marketplace plugin, rail plugin trong composer, Web Clipper, player âm thanh trong chat, thêm provider media.
- **Thiếu thao tác file**: không upload, không đính file vào chat, không menu file. Đây là chỗ người dùng vấp đầu tiên.

## 15 khoảng trống đáng làm nhất

Xếp theo giá trị cho người dùng chia công sức. Giờ là ước lượng cho một người đã quen codebase, chưa gồm review.

| # | Khoảng trống | Nơi trong app gốc | Giờ | Crate Rust |
|---|---|---|---|---|
| 1 | Đính file/ảnh vào chat: nút đính, paste ảnh, kéo thả (API upload đã có) | `ChatComposer.tsx`, `ComposerPlusMenu.tsx`, en:223 | 4–6 | od-ui |
| 2 | Conversation: đổi tên, xoá; thu gọn/resize pane chat | `ChatPane.tsx:3469`, en:2263-2271, en:833 | 2–3 | od-ui, od-server, od-store |
| 3 | "Files from this turn" có Open/Download, Copy tin nhắn, "Continue remaining tasks" | `FileOpsSummary.tsx`, en:3620-3675 | 3–4 | od-ui |
| 4 | Toggle Preview/Code (xem source có tô màu) | `FileViewer.tsx:2456,6962` | 3–4 | od-ui |
| 5 | Badge trạng thái run trên thẻ project (spec ghi P0) | `DesignsTab.tsx`, en:2015-2023 | 2–4 | od-ui, od-server, od-runs |
| 6 | Panel file: upload, menu dòng (open, download, copy path, delete), thư mục thu gọn được | `DesignFilesPanel.tsx`, en:2936-3043 | 6–8 | od-ui, od-server |
| 7 | Settings: form BYOK (provider, key, base URL), media provider, nút Test kết nối | `SettingsDialog.tsx` (execution, media), en:510-726 | 5–7 | od-ui, od-server, od-runtime |
| 8 | Viewer/editor Markdown (Editor/Split/Preview, auto-save) | `FileViewer.tsx:20463`, en:3131-3140 | 4–6 | od-ui |
| 9 | Speaker notes sửa và lưu; present fullscreen/tab mới | `FileViewer.tsx:14377,2413`, en:3308-3327 | 3–4 | od-ui |
| 10 | Zoom preview, chụp màn hình vào clipboard và vào chat | `FileViewer.tsx:2393,17308`, en:3164,3196 | 4–5 | od-ui, od-preview |
| 11 | Model picker thật (dropdown, reasoning effort) và đổi model ngay trong composer | `InlineModelSwitcher.tsx`, `AvatarMenu.tsx` | 4–6 | od-ui, od-runtime |
| 12 | Projects: tìm, lọc, sắp xếp, Cmd/Ctrl+K | `ProjectSearchModal.tsx`, en:1196-1240 | 4–6 | od-ui, od-store |
| 13 | ~~Trang Design systems: duyệt, kit view (showcase, tokens, DESIGN.md), đặt mặc định~~ (xong, B1-library) | `DesignSystemsTab.tsx`, `DesignKitView.tsx`, en:4489-4530 | 10–14 | od-ui, od-content, od-server |
| 14 | Thêm chip đầu ra: Document, Wireframe, Mobile app (skill đã có trong content) | `home-hero/chips.ts`, en:1060-1110 | 6–8 | od-ui (`intent.rs`), od-content |
| 15 | ~~Trang Plugins/Skills: danh sách, chi tiết, "Use" có form input~~ (xong, B1-library) | `PluginsView.tsx`, `PluginDetailView.tsx`, `PluginInputsForm.tsx` | 12–16 | od-ui, od-server, od-content |

Còn đáng cân nhắc sau 15 mục trên (tạo design system từ URL/GitHub đã xong ở B1-library): viewer live artifact (6–8 giờ, od-live), sửa automation và lịch preset (2–3 giờ, od-automation), tab browser tích hợp (lớn, cần Tauri webview).

## Bảng đầy đủ

| Nhóm | Tính năng (app gốc) | Điểm | Bản Rust: bằng chứng / thiếu gì |
|---|---|---|---|
| Home | Ô brief, Enter tạo project và gửi lượt đầu | 100 | `crates/od-ui/src/app/home.rs:123`; ảnh `(audit run) shots/home-light.png` |
| Home | Chip loại đầu ra (Prototype, Website clone, Wireframe, Mobile app, Slide deck, Document, Image, Video, Audio, Motion, HyperFrames, WebGL, Live artifact) | 50 | Chỉ Prototype và Slide deck (`crates/od-ui/src/intent.rs:9`) |
| Home | Chip hành động (Create design system, Create plugin, From Figma, From folder, From template) | 50 | Có "New design system…" (`home.rs:4`, `ds_create.rs:24`) và chọn template; không có Create plugin, From Figma; From folder chỉ ở trang Projects |
| Home | Chọn design system lúc tạo | 100 | `home.rs` picker; `(audit run) shots/home-light.png` |
| Home | Chọn template + gallery "Start from a template" | 100 | `home.rs:286`, 57 template, Show all; `(audit run) shots/home-decks-light.png` |
| Home | Tiêu đề xoay chữ, carousel placeholder, ví dụ prompt | 0 | Tiêu đề và placeholder tĩnh |
| Home | Menu "+" thêm ngữ cảnh (đính file, plugin, connector, MCP) | 0 | Không có |
| Home | Tìm ngữ cảnh bằng @ (file, plugin, skill, MCP) | 0 | Không có |
| Home | Picker media ở chân composer (Ratio, Duration, Resolution, Notes) | 0 | Không có (chỉ có 2 chip) |
| Home | Working directory picker (chọn, đổi, thư mục gần đây) | 0 | Không có trên Home; project luôn tạo trong data dir |
| Home | Thẻ "Recommended for you" | 0 | Không có |
| Home | Dải project gần đây (+ lọc owner/type) | 50 | `home.rs:243`; không có lọc |
| Home | Rail trái (Home, Projects, Automations, Plugins, Design systems, Integrations, thu gọn) | 50 | `crates/od-ui/src/app/mod.rs:203`: Home, Projects, Automations, Settings, thu gọn; thiếu Plugins, Design systems, Integrations |
| Home | Tab entry (Designs, Templates, Design systems, Connectors, Image/Video templates) | 50 | Chỉ có gallery template trên Home |
| Home | Message center (thông báo trong app) | 0 | Không có |
| Home | Onboarding (chọn agent, BYOK, test kết nối, trích brand) | 50 | `crates/od-ui/src/app/onboarding.rs:11`: chọn agent + Skip; thiếu BYOK, test, trích design system; `(audit run) shots/onboarding-light.png` |
| Home | Đổi theme, đổi ngôn ngữ | 100 | Settings ▸ Appearance, Language (`settings_page.rs:145`) |
| Home | Pet nổi | 0 | Không có (spec 1.5 ghi "cân nhắc BỎ", chưa chốt) |
| Home | GitHub Star, Discord, Get help, feature request | N/A | spec 1.6 BỎ (badge, community) |
| Home | What's new, privacy consent, khảo sát NPS, cloud sign-in, allowance, campaign | N/A | Update/telemetry/cloud/billing: BỎ |
| Projects | Lưới project có ảnh bìa | 100 | `crates/od-ui/src/app/projects.rs:157`; `(audit run) shots/projects-light.png` |
| Projects | Đổi tên, nhân bản, xoá có xác nhận | 100 | `projects.rs:199`, `projects.rs:281` |
| Projects | Badge trạng thái run (Running, Needs input, Completed…) | 0 | Không có; spec 1.1 ghi chấm trạng thái là P0 |
| Projects | Tag loại project | 50 | Chỉ Prototype/Deck/Project |
| Projects | Board (kanban) theo trạng thái | 0 | Không có |
| Projects | Chọn nhiều để xoá | 0 | Không có |
| Projects | Lọc, sắp xếp, list view | 0 | Không có |
| Projects | Tìm project và conversation (Cmd/Ctrl+K) | 0 | Không có |
| Projects | Panel tạo project (fidelity, nền tảng đích, speaker notes, animation, media options) | 50 | Nút "New project" có; không có tuỳ chọn |
| Projects | Mở folder làm project | 100 | `projects.rs:53-71`: dialog native trên desktop, ô đường dẫn trên web; menu Cmd+O `apps/desktop/src/main.rs:160` |
| Projects | Import ZIP (Claude Design) | 0 | Không có |
| Projects | Project instructions | 50 | API có `custom_instructions` (`crates/od-contracts/src/projects.rs:57`); UI không có |
| Projects | Live artifact badge trên thẻ | 0 | Không có |
| Projects | Recently viewed | 0 | Không có |
| Projects | Team space, invite, presence, sync thiết bị | N/A | Team/cloud: BỎ |
| Studio-chat | Stream lượt assistant: text, thinking, tool row, Markdown | 100 | `crates/od-ui/src/app/chat_view.rs:444-466`; `(audit run) shots/studio-light.png` |
| Studio-chat | Stop | 100 | `chat_view.rs` nút Stop → `ctx.cancel()` |
| Studio-chat | Danh sách conversation: tạo, đổi tên, xoá | 50 | `chat_view.rs:159-190`: dropdown + New; thiếu rename/delete |
| Studio-chat | Fork từ một tin | 100 | `chat_view.rs:403` |
| Studio-chat | Session mode (Ask/Plan/Design) | 100 | `crates/od-ui/src/app/agent_w3.rs` (Design/Chat/Plan) |
| Studio-chat | Đính file, upload trong composer | 0 | Daemon có `POST /api/projects/{id}/upload`; UI không có nút đính file, không nhận paste/drop ảnh |
| Studio-chat | Tham chiếu project khác, link local code | 0 | Không có |
| Studio-chat | Import Figma (.fig, URL) | 0 | Không có |
| Studio-chat | Import từ Library | 0 | Không có |
| Studio-chat | Đổi design system ngay trong composer | 0 | DS chỉ chọn lúc tạo (`studio.rs:291` đọc từ project) |
| Studio-chat | Palette @ mention | 0 | Không có |
| Studio-chat | Slash command (/mcp, /search) | 0 | Không có |
| Studio-chat | Inline model switcher (CLI/BYOK, agent, model) | 0 | Chỉ đổi trong Settings |
| Studio-chat | Queue: sửa, kéo thả, steer | 100 | `chat_view.rs:229-293` |
| Studio-chat | Quote to chat | 0 | Không có |
| Studio-chat | Trạng thái run (Working, Thinking) | 100 | `chat_view.rs:459` |
| Studio-chat | Todo list + plan pill "bước N/M" | 100 | `chat_view.rs:107,413`; `(audit run) shots/studio-light.png` |
| Studio-chat | Dòng sinh media, audio player | 0 | Không có |
| Studio-chat | "Files from this turn" (Open/Download) | 50 | Chỉ có dòng chữ "Wrote index.html (v1)"; không có nút mở/tải |
| Studio-chat | Copy tin nhắn | 0 | Không có |
| Studio-chat | "Continue remaining tasks" | 0 | Không có |
| Studio-chat | Question form | 100 | `crates/od-ui/src/question_form.rs`, `chat_view.rs:489` |
| Studio-chat | GenUI surface (Authorize, Accept all…) | 0 | Không có |
| Studio-chat | Next-step actions, suggested next steps | 0 | Không có (daemon không phát `next_steps`) |
| Studio-chat | Design toolbox follow-up (Match, Icons, Motion, Polish…) | 0 | Không có |
| Studio-chat | Design Jury (critique theater) | 50 | `agent_w3.rs:163,427`: bật, vòng, 5 vai, điểm; thiếu Interrupt, Replay |
| Studio-chat | Lỗi run, reconnect, retry | 50 | Báo "Lost the connection"; không có thẻ lỗi có nút Reconnect/Retry |
| Studio-chat | Xuất log/conversation | 100 | `agent_w3.rs:319-350` (Markdown, JSON) |
| Studio-chat | Memory của project (+ "Remember that…") | 100 | `agent_w3.rs:202`; `(audit run) shots/studio-memory-light.png` |
| Studio-chat | Side chat | 100 | `agent_w3.rs:724`; `(audit run) shots/studio-side-chat-light.png` |
| Studio-chat | Thu gọn/resize pane chat | 0 | Không có |
| Studio-chat | Cảnh báo deliverable sau run | 100 | `crates/od-ui/src/app/preview_tools.rs:1214` |
| Studio-chat | Thumbs feedback, share to community, support dialog, balance gate | N/A | Telemetry/share/community/billing: BỎ |
| Studio-preview | Preview HTML sandbox + live reload | 100 | `crates/od-ui/src/app/studio.rs:1170-1200`, project SSE `studio.rs:479` |
| Studio-preview | Toggle Preview / Code | 0 | Không có xem source trong preview |
| Studio-preview | Chọn thiết bị Desktop/Tablet/Mobile | 100 | `studio.rs:1118` |
| Studio-preview | Zoom in/out/reset | 0 | Không có |
| Studio-preview | Reload | 100 | `studio.rs` nút Reload preview |
| Studio-preview | Chụp màn hình vào clipboard | 0 | Không có |
| Studio-preview | Screenshot to chat | 50 | Draw ▸ Attach gửi kèm ảnh chụp trang; không có nút chụp riêng |
| Studio-preview | Comment mode: ghim, gửi kèm lượt sau | 100 | `crates/od-ui/src/app/preview_comments.rs:62`; `(audit run) shots/studio-comments-light.png` |
| Studio-preview | Tab Comments, board, trạng thái anchor cũ/mất, đính ảnh vào comment | 50 | Chip comment trên composer (`chat_view.rs:310`); thiếu tab danh sách, anchor state, ảnh |
| Studio-preview | Draw/Mark (pen, box, text, undo/redo) | 50 | `preview_tools.rs:1054`: pen, box, clear; thiếu text, undo/redo |
| Studio-preview | Manual edit (layers, Content/Style/Attributes/Html, style controls, upload ảnh, xoá phần tử) | 50 | Inspect + Edit text + Save to source (`preview_tools.rs:622,669`); thiếu layers, attributes, đổi ảnh, xoá phần tử |
| Studio-preview | Inspect panel | 100 | `preview_tools.rs:669`; `(audit run) shots/studio-inspect-light.png` |
| Studio-preview | Tweaks | 100 | `preview_tools.rs:776` |
| Studio-preview | Palette swap 5 preset | 100 | `preview_tools.rs:599` |
| Studio-preview | Lint anti-slop | 100 | `preview_tools.rs:856`; `(audit run) shots/studio-lint-light.png` |
| Studio-preview | Lịch sử phiên bản: xem, khôi phục | 100 | `crates/od-ui/src/app/versions.rs:28`; `(audit run) shots/studio-history-light.png` |
| Studio-preview | Deck: điều hướng, rail thumbnail | 100 | `crates/od-ui/src/app/deck.rs:28,108`; `(audit run) shots/studio-deck-light.png` |
| Studio-preview | Speaker notes: xem, sửa, lưu | 50 | `deck.rs:147` chỉ xem |
| Studio-preview | Present: trong tab / fullscreen / tab mới / presenter mode | 50 | `deck.rs:70` chỉ present trong tab, Esc thoát |
| Studio-preview | Markdown viewer/editor (Editor/Split/Preview) | 0 | File .md hiện thô qua iframe raw |
| Studio-preview | Live artifact viewer (Preview/Code/Data/Refresh history) | 0 | API `/api/live-artifacts/*` có; UI không có |
| Studio-preview | Viewer ảnh | 100 | `studio.rs:1094` |
| Studio-preview | Viewer PDF/Office/video/audio | 50 | Trình duyệt tự render PDF/video qua iframe raw; Office không trích text, không có nút Download/Open |
| Studio-preview | Finalize design package (DESIGN.md) | 50 | Route `/api/projects/{id}/handoff`; không có nút Finalize/Re-finalize |
| Studio-preview | Hand off: chọn editor, framework, copy prompt, Continue in CLI | 50 | `crates/od-ui/src/app/export_menu.rs:247-251`: Open project folder, Copy prompt; thiếu chọn editor/framework, phím Cmd+Shift+K |
| Studio-preview | Brand ready prompt | 0 | Không có |
| Studio-preview | Share link, visibility | N/A | Share/cloud: BỎ |
| Studio-files/tabs | Panel file (cây, breadcrumb) | 50 | `studio.rs:945`: danh sách thụt lề; không breadcrumb, không thu gọn thư mục |
| Studio-files/tabs | Upload, paste as text file, kéo thả | 0 | Không có trong UI |
| Studio-files/tabs | New sketch | 0 | Không có |
| Studio-files/tabs | New document (Markdown từ mẫu) | 0 | Không có |
| Studio-files/tabs | Thư mục: tạo, chuyển | 0 | Không có |
| Studio-files/tabs | Menu dòng: open in tab, download, copy path, delete | 0 | Không có |
| Studio-files/tabs | Chọn nhiều: tải ZIP, xoá | 0 | Không có |
| Studio-files/tabs | Nhóm/lọc theo loại, phân trang | 0 | Không có |
| Studio-files/tabs | Tạo design system từ project | 0 | Không có |
| Studio-files/tabs | Thanh tab file, đóng tab | 100 | `studio.rs:996` |
| Studio-files/tabs | Tab launcher "+" (mở file, tạo mới) | 50 | Nút + chỉ mở terminal; Cmd+T mở quick switcher |
| Studio-files/tabs | Tab terminal | 100 | `crates/od-ui/src/app/terminal.rs:265` (xterm.js); `(audit run) shots/studio-terminal-light.png` |
| Studio-files/tabs | Tab browser tích hợp | 0 | Không có |
| Studio-files/tabs | Pages list, tạo page | 0 | Không có |
| Studio-files/tabs | Focus workspace / Show chat | 0 | Không có |
| Studio-files/tabs | Quick switcher Cmd/Ctrl+P | 100 | `crates/od-ui/src/app/switcher.rs:14`; `(audit run) shots/studio-switcher-light.png` |
| Export | PDF | 100 | `export_menu.rs:32`, `crates/od-export`, `crates/od-render-cdp` |
| Export | PPTX ảnh | 100 | `export_menu.rs:34` |
| Export | PPTX editable | 0 | Không có (spec P2) |
| Export | ZIP project | 100 | `export_menu.rs:31` |
| Export | HTML standalone | 100 | `export_menu.rs:30` |
| Export | Export Markdown (file) | 0 | Chỉ có xuất conversation ra .md |
| Export | Export ảnh có chọn định dạng | 50 | Chỉ PNG |
| Export | Export JSX / preview as HTML | 0 | Không có |
| Export | Save as template | 0 | Không có |
| Export | MP4 HyperFrames | 50 | API `/export/mp4` + CLI `od export`; không có trong menu UI |
| Export | Open in new tab, copy path, download file | 0 | Không có |
| Export | Deploy Vercel/Cloudflare, social share, quick share | N/A | Deploy/share: BỎ |
| Design systems | Trang Design systems (Your systems, presets, lọc) | 100 | `crates/od-ui/src/app/ds_page.rs`: Của tôi / Preset có sẵn (đếm), tìm, lọc bề mặt và nhóm, danh sách + chi tiết; route `/design-systems` |
| Design systems | Chọn design system khi tạo project | 100 | `home.rs` |
| Design systems | Tạo DS từ mô tả thương hiệu | 100 | `crates/od-ui/src/app/ds_create.rs:24`; `(audit run) shots/home-design-system-dialog-light.png` |
| Design systems | Tạo DS từ URL, GitHub, file, Figma, paste DESIGN.md, copy DS có sẵn | 100 | `ds_create.rs` `CreateFlow`: website, GitHub, Figma, file đính kèm qua agent (`POST /api/design-systems/generate` có `urls/github/figma/files`); DESIGN.md hoặc biến CSS dán vào cài thẳng (`POST /api/design-systems/import`); chép bộ có sẵn (`/copy`). Figma đọc qua công cụ Figma của agent, không giải mã file `.fig` |
| Design systems | "Start from a brand" picker | 100 | `ds_create.rs` `BrandPicker`, `GET /api/design-systems/brands` (36 thương hiệu, ô chữ tắt thay favicon vì không mạng ngoài); chọn là thêm website làm nguồn |
| Design systems | Tiến trình sinh theo pha | 100 | Job có `phase` (sources, design, tokens, checking) đọc từ file agent đã ghi (`routes/w3/design_systems.rs`); UI hiện 4 bước |
| Design systems | Review (Looks good / Needs work), revision accept/reject | 100 | `ds_page.rs`: Ổn rồi = xuất bản; Cần sửa → agent sửa bản sao (`POST …/revisions`), duyệt Nhận/Bỏ (`POST …/revisions/:rid`) |
| Design systems | Kit view (Showcase, Tokens, DESIGN.md), brand detail, preview modal | 100 | Tab Kit (bảng màu, kiểu chữ, token contract, asset, trang preview), Showcase (`components.html` hoặc trang dựng từ token), Tokens, DESIGN.md; xem lớn trong modal (`GET …/kit`, `…/showcase`) |
| Design systems | Sửa/copy/lưu DESIGN.md, upload font/logo | 100 | `PUT …/design-md`, chép clipboard, `POST …/assets` (logo, ảnh, font vào `assets/`), xoá asset |
| Design systems | Default for new chats | 100 | `designSystemId` trong app-config; Home chọn sẵn (`home.rs`); chỉ preset hoặc bộ đã xuất bản |
| Design systems | Download DS, Edit with agent | 100 | `GET …/archive` (ZIP); `POST …/workspace` mở project trỏ thẳng vào thư mục package |
| Design systems | Import DS từ GitHub/shadcn trong Settings | 100 | Đặt ở luồng Tạo của trang Design systems: GitHub qua agent clone, theme shadcn/Tailwind dán vào chuyển thành token (`design_kit::tokens_from_css_vars`); không tải registry shadcn vì daemon không ra mạng |
| Design systems | Rebuild token contract, điểm chất lượng | 100 | `od-content/src/design_kit.rs` `score` (công thức gốc, 4 hạng), `rebuild_tokens` tạo revision chờ duyệt (`POST …/token-contract/rebuild`) |
| Design systems | Publish, share/sync to team | N/A | Publish/team: BỎ |
| Plugins/Skills | Danh sách, chi tiết plugin | 100 | `crates/od-ui/src/app/plugins_page.rs`: Chính thức/Cá nhân, tìm, nhóm; chi tiết: brief mẫu, input, workflow, context, quyền, nguồn |
| Plugins/Skills | Dùng plugin (Use, Remix) | 100 | `POST /api/plugins/:id/use` tạo project gắn plugin (mọi run trong project áp plugin, `library/plugins.rs::project_plugin_request`); Remix ví dụ ở trang Templates |
| Plugins/Skills | Form input plugin | 100 | `plugins_page.rs` `InputsForm`: select, number, boolean, text, string; bắt buộc có dấu *, nút Dùng khoá tới khi đủ |
| Plugins/Skills | Thêm plugin (từ prompt, link, folder), install/uninstall | 100 | Tạo bằng agent và từ link (`POST /api/plugins/author`, agent tải về rồi `od plugin install`), cài từ thư mục (`POST /api/plugins/install`, CLI `od plugin install`), gỡ (`/uninstall`) |
| Plugins/Skills | Registry sources, trust, marketplace | 0 | Không có (spec P2): registry và marketplace tải manifest từ mạng, daemon không ra mạng (decisions.md); trust chỉ có ở cổng capability của CLI (`od plugin apply`, exit 66) |
| Plugins/Skills | Danh sách, chi tiết skill | 100 | Tab Skill của trang Plugins: danh sách, chi tiết nguyên văn SKILL.md, Dùng (Home chọn sẵn skill) |
| Plugins/Skills | Sửa skill (tạo, sửa, xoá, user override) | 100 | `POST /api/skills`, `PUT/DELETE /api/skills/:id` (`od-content/src/user_content.rs`); sửa skill có sẵn tạo bản riêng giữ metadata `od:` |
| Plugins/Skills | Plugin rail trong composer | 0 | Không có: composer chat thuộc track Studio; app gốc cũng tắt rail (`ChatComposer.tsx` `showRail={false}`), plugin vào chat qua menu công cụ |
| Plugins/Skills | Publish/contribute plugin, share with team | N/A | Publish/community/team: BỎ |
| Templates/Examples | Gallery có lọc (loại, surface, nhóm ngành) | 100 | `crates/od-ui/src/app/templates_page.rs`: loại, nền tảng, ngành, tìm |
| Templates/Examples | Thẻ template có preview sống | 100 | `crates/od-ui/src/app/lazy_frame.rs:17`, thumbnail iframe |
| Templates/Examples | Dùng template để tạo | 100 | `home.rs:324` |
| Templates/Examples | Preview modal (fullscreen, Remix, export) | 100 | Toàn màn hình, Remix (`POST /api/design-templates/:id/remix`), mở tab mới, tải HTML, Dùng template; PDF/ZIP qua Export của project sau Remix |
| Templates/Examples | Prompt template ảnh/video (Copy prompt) | 100 | Tab Prompt ảnh/video: Chép prompt, Sinh (mở trang Media điền sẵn) |
| Templates/Examples | Template do user lưu | 100 | `/api/templates` (bảng `templates`): lưu project, liệt kê, dùng, xoá (tab Template của tôi) |
| Templates/Examples | Community gallery, share template | N/A | Community/share: BỎ |
| Settings | Chọn agent + rescan | 100 | `crates/od-ui/src/app/settings_page.rs`; `(audit run) shots/settings-light.png` |
| Settings | Model, reasoning effort, service tier | 50 | Ô text Model; thiếu dropdown, reasoning, service tier |
| Settings | Advanced: proxy, đường dẫn tuỳ chỉnh | 0 | Không có |
| Settings | Test kết nối agent | 0 | Không có |
| Settings | BYOK: preset provider, key, base URL, fetch models | 50 | Daemon chạy BYOK (Anthropic + OpenAI-compatible); chỉ cấu hình qua `PUT /api/app-config` (README), UI không có |
| Settings | Quyền agent (Automatic / Ask first) | 100 | `settings_page.rs:119` |
| Settings | Ngôn ngữ | 50 | en + vi (`crates/od-ui/src/i18n.rs`); app gốc 19 locale |
| Settings | Theme | 100 | System/Light/Dark `settings_page.rs:145` |
| Settings | Thông báo: âm thanh, desktop notification | 0 | Không có |
| Settings | Project locations | 0 | Không có |
| Settings | Global rules / instructions | 0 | Không có |
| Settings | Memory toàn cục (loại, hooks, import, MEMORY.md) | 50 | Chỉ memory theo project |
| Settings | Media providers (key, base URL) | 50 | API `/api/media/config` + CLI; UI không có |
| Settings | About (version, export diagnostics cục bộ, reset onboarding) | 0 | Không có |
| Settings | Menu cài đặt nhanh (avatar menu) | 0 | Không có |
| Settings | Labs, privacy, account, wallet, update | N/A | Labs gửi feedback; còn lại telemetry/cloud/billing/update: BỎ |
| Agent runtimes | Claude Code (stream, resume, steer) | 100 | `crates/od-runtime`; steer ở UI `studio.rs:346` |
| Agent runtimes | Codex | 100 | `crates/od-runtime`; README bảng agent |
| Agent runtimes | Cursor, OpenCode, Copilot, Qwen | 100 | README bảng agent; `(audit run) shots/settings-light.png` |
| Agent runtimes | Họ ACP (Kimi, Hermes, Kiro, Kilo, Vibe, Trae, Reasonix, Devin, DeepSeek Harness) | 100 | README; `(audit run) shots/settings-light.png` |
| Agent runtimes | CLI khác (Aider, Amp, Pi, Qoder, MiMo, Codebuddy, AtomCode, Grok Build, Antigravity, DeepSeek TUI) | 0 | Không có adapter |
| Agent runtimes | Phát hiện agent + Refresh | 100 | `GET /api/agents`, `onboarding.rs:77` |
| Agent runtimes | Test kết nối | 0 | Không có |
| Agent runtimes | BYOK preset (26 provider) | 50 | 2 giao thức, base URL tuỳ chỉnh phủ được nhiều provider OpenAI-compatible |
| Agent runtimes | AMR cloud | N/A | Cloud: BỎ |
| MCP | MCP server `od mcp` (stdio) | 100 | `crates/od-cli/src/mcp_cmd.rs:1`, `crates/od-mcp` |
| MCP | Cài MCP vào agent (tab theo client, copy command) | 50 | CLI `od mcp install <agent>`; UI không có |
| MCP | MCP server ngoài (stdio/HTTP, args, test) | 100 | `crates/od-ui/src/app/mcp_settings.rs:36`; `(audit run) shots/settings-mcp-light.png` |
| MCP | OAuth MCP | 100 | `mcp_settings.rs` Sign in / Sign out; route `/oauth/start` |
| MCP | Catalog mẫu MCP | 0 | Không có |
| MCP | Ghi chú forward tới agent nào | 0 | Không có |
| MCP | Connectors (Composio) | 0 | Không có (spec P2 "cân nhắc BỎ", chưa chốt) |
| MCP | Use everywhere (hướng dẫn CLI + MCP) | 0 | Không có |
| Media | Provider media | 50 | `crates/od-media/src/provider.rs`: OpenAI, fal, ElevenLabs (base URL tuỳ chỉnh nên dùng được gateway tương thích OpenAI); cấu hình key và base URL ở tab Provider của trang Media; app gốc 16 provider tích hợp thật |
| Media | Project ảnh/video/audio trong UI (model, aspect, voice) | 100 | `crates/od-ui/src/app/media_page.rs` tab Sinh: ảnh/video/âm thanh, model, tỉ lệ, thời lượng, giọng, lưu vào project; qua hàng đợi `/api/media/tasks` |
| Media | Audio player trong chat | 50 | Có player ở thư viện và hàng đợi Media; trong chat chưa có (khung chat Studio thuộc track khác) |
| Media | Library (lọc, upload, Chat to design) | 100 | `GET /api/library/assets`, `POST /api/library/upload`, `POST /api/library/chat`; lọc loại, tìm, project, xem lớn, tải, xoá |
| Media | Web Clipper | 0 | Không có: extension trình duyệt riêng (`clipper/` app gốc), cần endpoint ingest nhận origin extension; UI Library của app gốc đang tắt (`LIBRARY_UI_VISIBLE = false`) |
| Media | HyperFrames → MP4 | 100 | Tab HyperFrames ra MP4 của trang Media (`POST /api/projects/:id/export/mp4`, lưu vào project) |
| Automations | Trang Automations | 100 | `crates/od-ui/src/app/automations.rs:80`; `(audit run) shots/automations-light.png` |
| Automations | Tạo automation (tên, prompt, lịch, project) | 50 | Lịch nhập cron tay, không preset Hourly/Daily, không timezone, không "tạo project mới mỗi lần" |
| Automations | Thao tác dòng: Run now, bật/tắt, xoá, mở conversation, sửa | 50 | `automations.rs:196`; thiếu Edit |
| Automations | Lịch sử run | 100 | `automations.rs` RoutineRow |
| Automations | Crystallize, evolution proposals | 0 | Không có |
| Automations | Template automation | 0 | Không có |
| Automations | Orbit (tóm tắt connector hằng ngày) | 0 | Không có |
| Automations | Refresh live artifact | 0 | API có, UI không có |
| Desktop shell | Cửa sổ chính, daemon trong process | 100 | `apps/desktop/src/main.rs:48` |
| Desktop shell | Menu App/Edit/View/Window | 100 | `Menu::default` `apps/desktop/src/main.rs:161` |
| Desktop shell | Menu Help (Documentation, Report Issue) | 0 | Không có |
| Desktop shell | Folder picker native | 100 | `apps/desktop/src/main.rs:105` |
| Desktop shell | Mở thư mục trong Finder | 100 | Handoff ▸ Open project folder |
| Desktop shell | Export PDF/ảnh bằng engine trình duyệt | 100 | Chrome có sẵn qua CDP (decisions.md #2) |
| Desktop shell | Splash | 0 | Không có |
| Desktop shell | Đồng bộ theme native | 0 | Không có |
| Desktop shell | Cửa sổ pet | 0 | Không có |
| Desktop shell | Auto-update, invite deep link, crash diagnostics, Develop menu AMR | N/A | decisions.md: auto-update bỏ; cloud/telemetry BỎ |
| Phím tắt | Cmd/Ctrl+, mở Settings | 0 | Không có |
| Phím tắt | Cmd/Ctrl+K tìm project | 0 | Không có |
| Phím tắt | Cmd/Ctrl+B thu gọn rail | 0 | Chỉ có nút |
| Phím tắt | Cmd/Ctrl+P quick switcher | 100 | `studio.rs:713` |
| Phím tắt | Cmd/Ctrl+T tab mới | 50 | Mở quick switcher thay vì tab launcher |
| Phím tắt | Cmd/Ctrl+W đóng tab | 100 | `studio.rs:717` |
| Phím tắt | Ctrl+Tab / Ctrl+Shift+Tab | 100 | `studio.rs:737` |
| Phím tắt | Ctrl+PageUp/PageDown, Cmd+Opt+←/→ | 0 | Không có |
| Phím tắt | Cmd/Ctrl+1…9 | 100 | `studio.rs:725` |
| Phím tắt | Cmd/Ctrl+Shift+K Continue in CLI | 0 | Không có |
| Phím tắt | Enter / Shift+Enter gửi, xuống dòng | 100 | `chat_view.rs:121` |
| Phím tắt | @ và / trong composer | 0 | Không có |
| Phím tắt | Cmd/Ctrl+Enter gửi comment | 50 | Comment gửi bằng Enter (`preview_comments.rs:177`) |
| Phím tắt | Cmd/Ctrl+Z trong draw | 0 | Không có undo |
| Phím tắt | Deck: →, ←, PageUp/Down, Home, End, R | 50 | `studio.rs:755-762`; thiếu R |
| Phím tắt | Esc đóng modal, present, mode | 100 | `studio.rs:745` |
| Phím tắt | Phím trong DS kit (E/C/U/R/⌫) | 100 | `ds_page.rs` (E sửa, C chép, U tải lên, R dựng lại token, ⌫ xoá asset đang chọn) |
