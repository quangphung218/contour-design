# Parity Nét (open-design-rs) so với OpenDesign — đo lần cuối tại main `94dd674`

- Bản Rust: `main` HEAD `94dd674` (merge `waveC-integrate`, 2026-10-24): đợt C gồm C1-preview, C2-chat-home, C3-runtimes, C4-depth.
- Hai mốc so sánh: **40,7%** (`docs/parity.md` bản `8c9df6a`, 215 dòng chấm) và **90,9%** (bản `eb4eee2` đo tại `dc7938a` sau đợt B, 214 dòng chấm). Cùng danh mục, cùng cách chấm, cùng thứ tự dòng.
- App gốc: `~/Documents/GitHub/open-design` (chỉ đọc).
- Cách chấm: **100** đủ, **50** một phần (ghi thiếu gì), **0** không có, **N/A** bỏ có chủ đích. % nhóm là trung bình điểm, không tính N/A. Mỗi dòng nặng như nhau. Chỉ có ở API/CLI, không có trên UI: 50.
- Thay đổi danh mục: theo quyết định đã chốt (Connectors, Orbit, Figma, marketplace, pet, browser tab, Web Clipper, xAI OAuth, auto-update là N/A), thêm **7 dòng N/A**: #18 Pet, #43 Import Figma, #108 Tab browser, #142 Registry/marketplace, #191 Web Clipper, #199 Orbit, #209 Cửa sổ pet. Connectors (#185) và auto-update (#210) đã N/A từ trước. Dòng #3 (chip hành động) và #14 (tab entry) có một phần là Figma/Connectors: phần đó bỏ, dòng chấm trên phần còn lại. Vì vậy "Bây giờ" chấm **207 dòng**, đợt B chấm 214.
- Chạy thật: daemon `od` debug (`~/.cache/odrs-target-open-design-rs/debug/od`, build 08:52 từ `94dd674`, `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2`) + bundle UI build bằng `trunk build --dist` ra thư mục run + mock agent, data dir tạm do từng kịch bản tự tạo và xoá. Chạy 10 kịch bản Playwright của repo (y như `crates/od-ui/e2e/run.sh`) từ bản copy ngoài repo, tuần tự: studio, design-system, library, manage, chat, files, preview, c2, c3-runtimes, c4-depth. **10/10 PASS**, `externalRequests: 0` ở mọi kịch bản có đếm. 161 ảnh (light và dark) ở `shots/`, log ở `logs/`.
- Bằng chứng ghi `e2e <kịch bản> bước N` là đã chạy thật trong lần đo này; còn lại là đọc code (file:dòng tại `94dd674`). Số dòng của các dòng không đổi điểm được map tự động từ `dc7938a` sang `94dd674` qua hunk của `git diff` (114 dòng có số dòng dịch), đã đối chiếu tay 22 chỗ.

## Tổng hợp

| Nhóm | Số mục chấm | Đủ | Một phần | Không có | N/A | 40,7% (`8c9df6a`) | 90,9% (đợt B) | Bây giờ | Tăng so với đợt B |
|---|---|---|---|---|---|---|---|---|---|
| Projects | 14 | 14 | 0 | 0 | 1 | 32 | 100 | 100 | 0 |
| Design systems | 13 | 13 | 0 | 0 | 1 | 19 | 100 | 100 | 0 |
| Templates/Examples | 6 | 6 | 0 | 0 | 1 | 50 | 100 | 100 | 0 |
| MCP | 7 | 7 | 0 | 0 | 1 | 44 | 100 | 100 | 0 |
| Phím tắt | 17 | 17 | 0 | 0 | 0 | 44 | 100 | 100 | 0 |
| Studio-preview | 26 | 26 | 0 | 0 | 1 | 60 | 94 | 100 | +6 |
| Studio-files/tabs | 15 | 15 | 0 | 0 | 1 | 25 | 94 | 100 | +6 |
| Studio-chat | 31 | 31 | 0 | 0 | 2 | 44 | 88 | 100 | +12 |
| Plugins/Skills | 7 | 7 | 0 | 0 | 2 | 12 | 88 | 100 | +12 |
| Agent runtimes | 8 | 8 | 0 | 0 | 1 | 69 | 88 | 100 | +12 |
| Settings | 15 | 15 | 0 | 0 | 1 | 37 | 87 | 100 | +13 |
| Desktop shell | 8 | 8 | 0 | 0 | 2 | 56 | 83 | 100 | +17 |
| Automations | 7 | 7 | 0 | 0 | 1 | 38 | 81 | 100 | +19 |
| Home | 17 | 17 | 0 | 0 | 3 | 39 | 81 | 100 | +19 |
| Media | 5 | 5 | 0 | 0 | 1 | 17 | 75 | 100 | +25 |
| Export | 11 | 10 | 1 | 0 | 1 | 45 | 91 | 95 | +4 |
| **Tổng** | **207** | **206** | **1** | **0** | **20** | **40,7** | **90,9** | **99,8** | **+8,9** |

Đọc nhanh:
- **Tăng từ 90,9% lên 99,8%** (206 dòng 100, 1 dòng 50, 0 dòng 0). 15/16 nhóm đạt 100%.
- So cùng danh mục: nếu bỏ 7 dòng mới N/A khỏi bản đợt B thì đợt B là 93,7% (19.400/207). Phần tăng thật do code đợt C là **+6,1 điểm**; phần còn lại do chốt N/A.
- **Còn 1 dòng dưới 100**: #119 Export JSX. Đợt C làm Export as JSX nhưng thiếu "Export React component as HTML" và preview render component (app gốc có cả hai).
- Dòng #177 (BYOK preset) giữ 100 như đợt B nhưng còn thiếu Azure OpenAI (25/26). Chấm chặt thì là 50, tổng thành 99,5%.
- 5/6 "dòng mới" ngoài danh mục giờ đạt 100; xAI OAuth là N/A.

## Dòng còn dưới 100

| # | Dòng | Điểm | Thiếu gì | Giờ | Ghi chú |
|---|---|---|---|---|---|
| 1 | Export: Export JSX / preview as HTML (#119) | 50 | "Export React component as HTML" (app gốc `apps/web/src/runtime/exports.ts:855`, `react-component.ts:9`); preview render file `.jsx/.tsx` thành component (`FileViewer.tsx:6910`). Bản Rust mở `.jsx/.tsx` như code | 4–6 | Cần dựng srcdoc chạy React (vendored, không CDN) để giữ luật không ra mạng |
| — | Agent runtimes: BYOK preset (#177), đang chấm 100 | (100) | Preset Azure OpenAI (protocol `azure`: deployment, `api-version`) | 2–3 | Lệch cách chấm có từ đợt B; ghi để Atlat quyết giữ 100 hay hạ 50 |

## Dòng mới (app gốc có, danh mục cũ thiếu; không tính vào %)

| Nhóm | Tính năng (app gốc) | Đợt B | Bây giờ | Bản Rust tại `94dd674` |
|---|---|---|---|---|
| Studio-preview | Gợi ý một lần khi có artifact đầu tiên (`FirstArtifactHint.tsx`) | 0 | 100 | `crates/od-ui/src/app/first_hint.rs:61-71`; e2e preview bước 1; `shots/c1/first-artifact-hint-light.png` |
| Design systems | Banner thiếu file font thương hiệu (`MissingBrandFontsBanner.tsx`) | 0 | 100 | `crates/od-ui/src/app/font_banner.rs:33-42` (Upload, Keep), `crates/od-ui/src/brand_fonts.rs`; e2e c2 bước 11; `shots/c2-font-banner-light.png` |
| Studio-chat | Toast khi lưu memory (`MemoryToast.tsx`) | 0 | 100 | `crates/od-ui/src/app/memory_settings.rs:391`, `crates/od-ui/src/memory_toast.rs`; e2e c2 bước 5; `shots/c2-memory-toast-light.png` |
| Agent runtimes | Dialog cài DeepSeek Harness (`DeepSeekHarnessSetupDialog.tsx`) | 50 | 100 | `crates/od-ui/src/app/dsh_setup.rs` lệnh cài `dsh`, profile `open-design`, guide đóng gói, phát hiện lại qua `POST /api/agents/:id/detect`; không đóng gói `@open-design/dsh-runtime` (khác có chủ đích). e2e c3 bước 2, 6; `shots/c3/shots/dsh-setup-dialog-light.png` |
| Settings | Đăng nhập OAuth xAI (`XaiOAuthControl.tsx`) | 0 | N/A | xAI OAuth: N/A (quyết định đã chốt); preset xAI dùng key vẫn có |
| Studio | Gợi ý khôi phục workspace project (`ProjectWorkspaceRecoveryTip.tsx`) | 0 | 100 | `crates/od-ui/src/app/recovery_tip.rs:31`; e2e preview bước 5; `shots/c1/workspace-recovery-tip-light.png` |

## Kết quả chạy thật đợt C

| Mục đợt C | Kịch bản, bước | Kết quả | Ảnh |
|---|---|---|---|
| Manual edit | preview 3 | PASS: content, link, style, attributes, html, upload ảnh; xoá; undo/redo (undo đè thay đổi mới hơn bị từ chối 409) | `shots/c1/manual-edit-*` |
| Tab Comments | preview 4 | PASS: danh sách, ảnh, anchored/stale/lost, resolve/reopen, ảnh gửi kèm lượt | `shots/c1/comments-*` |
| Tô màu code | preview 2 | PASS: HTML+CSS+JS, CSS, JS, Markdown | `shots/c1/code-view-*` |
| Library trong composer | c2 3–4 | PASS: "+" trên Home, "@" và "+" trong Studio | `shots/c2-library-picker-*` |
| Onboarding trích brand | c2 1 | PASS | `shots/c2-onboarding-brand-*` |
| Memory (Settings, toast) | c2 5, 9 | PASS: hooks, import, MEMORY.md, công tắc tổng; toast | `shots/c2-settings-memory-*`, `shots/c2-memory-toast-*` |
| Jury Interrupt/Replay | c2 7 | PASS | `shots/c2-jury-*` |
| Dòng tiến trình media | c2 6 | PASS: 2 ảnh, 1 lỗi có Retry | `shots/c2-media-row-*` |
| Help menu / Quick menu | c2 10 | PASS (bản web); menu native Tauri chỉ đọc code | `shots/c2-quick-menu-*`, `shots/c2-docs-*` |
| Export JSX | c2 8 | PASS (file `.tsx` tải về đúng nguồn) | — |
| Runtimes mới | c3 1, 3 | PASS: phát hiện agent mới; một lượt Qoder ra thinking + text | `shots/c3/shots/agent-needs-setup-*` |
| Media provider | c3 4–5 | PASS: mỗi provider mới có thẻ; research qua Tavily (mock) | `shots/c3/shots/settings-media-providers-*` |
| DeepSeek dialog | c3 2, 6 | PASS: dialog, guide mở được khi chưa xong onboarding | `shots/c3/shots/dsh-setup-*` |
| 20 ngôn ngữ | c4 1–2 | PASS: picker 20 mục; 19 ngôn ngữ × 12 màn không tràn layout; ar/fa RTL; pt-PT trình duyệt → pt-BR | `shots/c4/settings-language-de-*`, `shots/c4/home-ar-*`, `shots/c4/automations-ja-*` |
| Crystallize | c4 3 | PASS: crystallize một run, proposal Apply | `shots/c4/automations-proposals-*` |
| GenUI | c4 4–5 | PASS: 4 surface, câu trả lời được nhớ cho lượt sau | `shots/c4/chat-genui-*` |

Lần chạy đầu c4-depth FAIL ở bước 1 ("timed out waiting for German Home link") vì bản copy ngoài repo thiếu `../locales/` mà script đọc (`c4-depth.e2e.mjs:58`). Không phải lỗi app: thêm thư mục locales cạnh bản copy thì PASS. Log lần lỗi: `logs/c4-depth.run1-fail.log`, ảnh `shots/c4-run1-failure-missing-locales.png`.

## Bảng đầy đủ

| # | Nhóm | Tính năng (app gốc) | 40,7% | 90,9% | Bây giờ | Bằng chứng (HEAD 94dd674) / thiếu gì |
|---|---|---|---|---|---|---|
| 1 | Home | Ô brief, Enter tạo project và gửi lượt đầu | 100 | 100 | 100 | `crates/od-ui/src/app/home.rs:412` (nút tạo), Enter gửi lượt đầu; e2e chat bước 6, studio bước 2 |
| 2 | Home | Chip loại đầu ra (Prototype, Website clone, Wireframe, Mobile app, Slide deck, Document, Image, Video, Audio, Motion, HyperFrames, WebGL, Live artifact) | 50 | 100 | 100 | 13 chip `crates/od-ui/src/intent.rs:13`; `crates/od-ui/src/app/home.rs:522,540`; e2e chat bước 3; `(audit run) shots/b3-home-light.png` |
| 3 | Home | Chip hành động (Create design system, Create plugin, From Figma, From folder, From template) | 50 | 50 | 100 | Create design system, Create plugin (`crates/od-ui/src/app/home.rs:436`), From folder (`home.rs:850`), template; From Figma **N/A** (Figma đã chốt N/A), nên dòng chấm trên phần còn lại |
| 4 | Home | Chọn design system lúc tạo | 100 | 100 | 100 | `crates/od-ui/src/app/home.rs:353` |
| 5 | Home | Chọn template + gallery "Start from a template" | 100 | 100 | 100 | `crates/od-ui/src/app/home.rs:382,975`; e2e studio bước 2b |
| 6 | Home | Tiêu đề xoay chữ, carousel placeholder, ví dụ prompt | 0 | 100 | 100 | `crates/od-ui/src/app/home.rs:308` tiêu đề xoay, `intent.rs:307` kịch bản placeholder, ví dụ prompt `home.rs:681`; e2e chat bước 2 |
| 7 | Home | Menu "+" thêm ngữ cảnh (đính file, plugin, connector, MCP) | 0 | 100 | 100 | `crates/od-ui/src/app/composer.rs:1005` PlusMenu (đính file, plugin, skill, MCP, thư mục); connector N/A; e2e chat bước 4–6 |
| 8 | Home | Tìm ngữ cảnh bằng @ (file, plugin, skill, MCP) | 0 | 100 | 100 | `crates/od-ui/src/app/composer.rs:717` palette @, `crates/od-ui/src/mention.rs`; e2e chat bước 5 |
| 9 | Home | Picker media ở chân composer (Ratio, Duration, Resolution, Notes) | 0 | 100 | 100 | `crates/od-ui/src/app/home.rs:609` MediaBar (model, ratio, resolution, duration, notes `home.rs:647`); e2e chat bước 3 |
| 10 | Home | Working directory picker (chọn, đổi, thư mục gần đây) | 0 | 100 | 100 | `crates/od-ui/src/app/home.rs:456,780-817` chọn/đổi/gần đây/bỏ; e2e chat bước 4 |
| 11 | Home | Thẻ "Recommended for you" | 0 | 0 | 100 | `crates/od-ui/src/app/home_tabs.rs:303-322` Recommended (Start, Next, Dismiss), gắn ở `home.rs:471`; e2e c2 bước 2; `shots/c2-home-recommended-light.png` |
| 12 | Home | Dải project gần đây (+ lọc owner/type) | 50 | 100 | 100 | `crates/od-ui/src/app/projects.rs:684-688` dải gần đây + lọc loại; lọc owner không áp dụng vì không có team (N/A); e2e manage bước 6; `(audit run) shots/manage/shots/home-recent-light.png` |
| 13 | Home | Rail trái (Home, Projects, Automations, Plugins, Design systems, Integrations, thu gọn) | 50 | 100 | 100 | `crates/od-ui/src/app/mod.rs:460-473`: Home, Projects, Templates, Design systems, Plugins, Media, Automations, Integrations, Inbox, Settings + thu gọn |
| 14 | Home | Tab entry (Designs, Templates, Design systems, Connectors, Image/Video templates) | 50 | 50 | 100 | `crates/od-ui/src/app/home_tabs.rs:19-25,103-114` tab Designs, Templates, Design systems, Image, Video (nhớ tab); Connectors N/A; e2e c2 bước 2 |
| 15 | Home | Message center (thông báo trong app) | 0 | 100 | 100 | `crates/od-ui/src/app/inbox.rs:109` Message center bản local (run xong/lỗi/huỷ, đã đọc, archive); thông báo từ platform ở xa không áp dụng; e2e manage bước 6 |
| 16 | Home | Onboarding (chọn agent, BYOK, test kết nối, trích brand) | 50 | 50 | 100 | `crates/od-ui/src/app/onboarding.rs:25-27,79`, `onboarding_brand.rs:91-130` bước 2 trích design system từ site thương hiệu; e2e c2 bước 1; `shots/c2-onboarding-brand-light.png` |
| 17 | Home | Đổi theme, đổi ngôn ngữ | 100 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:363` ngôn ngữ, theme ở Settings ▸ Appearance |
| 18 | Home | Pet nổi | 0 | 0 | N/A | Pet: N/A (quyết định đã chốt) |
| 19 | Home | GitHub Star, Discord, Get help, feature request | N/A | N/A | N/A | BỎ (badge, community) |
| 20 | Home | What's new, privacy consent, khảo sát NPS, cloud sign-in, allowance, campaign | N/A | N/A | N/A | BỎ (update/telemetry/cloud/billing) |
| 21 | Projects | Lưới project có ảnh bìa | 100 | 100 | 100 | `crates/od-ui/src/app/projects.rs:534,808`; `(audit run) shots/manage/shots/projects-grid-light.png` |
| 22 | Projects | Đổi tên, nhân bản, xoá có xác nhận | 100 | 100 | 100 | `crates/od-ui/src/app/project_actions.rs:130-145` menu project (đổi tên, nhân bản), xoá có xác nhận |
| 23 | Projects | Badge trạng thái run (Running, Needs input, Completed…) | 0 | 100 | 100 | `crates/od-ui/src/app/projects.rs:638` StatusBadge (Running, Needs input, Completed…); e2e manage bước 2 |
| 24 | Projects | Tag loại project | 50 | 100 | 100 | `crates/od-ui/src/app/manage.rs:114` 6 tag (Live artifact, Web clone, Slide, Media, Prototype, Other), `projects.rs:851` |
| 25 | Projects | Board (kanban) theo trạng thái | 0 | 100 | 100 | `crates/od-ui/src/app/projects.rs:988-1024` board theo trạng thái; `(audit run) shots/manage/shots/projects-board-light.png` |
| 26 | Projects | Chọn nhiều để xoá | 0 | 100 | 100 | `crates/od-ui/src/app/projects.rs:464-495` chọn nhiều, xoá có xác nhận; e2e manage |
| 27 | Projects | Lọc, sắp xếp, list view | 0 | 100 | 100 | `crates/od-ui/src/app/projects.rs:433` lọc loại, `:444` sắp xếp, `:529` list view; `(audit run) shots/manage/shots/projects-list-light.png` |
| 28 | Projects | Tìm project và conversation (Cmd/Ctrl+K) | 0 | 100 | 100 | Cmd/Ctrl+K `crates/od-ui/src/app/mod.rs:266` → `palette.rs:1` (project, trang, file); tìm cả conversation/tin nhắn `projects.rs:424,576` |
| 29 | Projects | Panel tạo project (fidelity, nền tảng đích, speaker notes, animation, media options) | 50 | 100 | 100 | `crates/od-ui/src/app/new_project.rs:132-321`: loại, nền tảng, fidelity, speaker notes, animation, media; `(audit run) shots/manage/shots/new-project-panel-light.png` |
| 30 | Projects | Mở folder làm project | 100 | 100 | 100 | `crates/od-ui/src/app/projects.rs:366,388-400`; Home `home.rs:850` |
| 31 | Projects | Import ZIP (Claude Design) | 0 | 100 | 100 | `crates/od-ui/src/app/projects.rs:345-351` import ZIP Claude Design; e2e manage |
| 32 | Projects | Project instructions | 50 | 100 | 100 | `crates/od-ui/src/app/projects.rs:892-965` dialog instructions; `(audit run) shots/manage/shots/project-instructions-light.png` |
| 33 | Projects | Live artifact badge trên thẻ | 0 | 100 | 100 | `crates/od-ui/src/app/projects.rs:649-658` LiveBadges |
| 34 | Projects | Recently viewed | 0 | 100 | 100 | `crates/od-ui/src/app/projects.rs:414` Recently viewed, ghi nhận ở `mod.rs` ViewTracker |
| 35 | Projects | Team space, invite, presence, sync thiết bị | N/A | N/A | N/A | Team/cloud: BỎ |
| 36 | Studio-chat | Stream lượt assistant: text, thinking, tool row, Markdown | 100 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:568`; e2e studio bước 3 |
| 37 | Studio-chat | Stop | 100 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:312` |
| 38 | Studio-chat | Danh sách conversation: tạo, đổi tên, xoá | 50 | 100 | 100 | `crates/od-ui/src/app/chat_ops.rs:100-167` đổi tên, xoá có xác nhận; `chat_view.rs:376` New; e2e chat bước 15 |
| 39 | Studio-chat | Fork từ một tin | 100 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:593` |
| 40 | Studio-chat | Session mode (Ask/Plan/Design) | 100 | 100 | 100 | `crates/od-ui/src/app/agent_w3.rs:151` |
| 41 | Studio-chat | Đính file, upload trong composer | 0 | 100 | 100 | `crates/od-ui/src/app/composer.rs:778-887`, `:978` nút đính, paste, kéo thả; e2e chat bước 7 |
| 42 | Studio-chat | Tham chiếu project khác, link local code | 0 | 100 | 100 | `crates/od-ui/src/app/chat_ops.rs:470-611` Link local code, Reference project; e2e chat bước 17 |
| 43 | Studio-chat | Import Figma (.fig, URL) | 0 | 0 | N/A | Figma: N/A (quyết định đã chốt) |
| 44 | Studio-chat | Import từ Library | 0 | 0 | 100 | `crates/od-ui/src/app/library_picker.rs:150-208` (tìm, lọc loại, import), Home `home.rs:295`, Studio `chat_view.rs:165`, @ trong `mention.rs`; e2e c2 bước 3–4; `shots/c2-library-picker-light.png` |
| 45 | Studio-chat | Đổi design system ngay trong composer | 0 | 100 | 100 | `crates/od-ui/src/app/chat_ops.rs:638`; e2e chat bước 11 |
| 46 | Studio-chat | Palette @ mention | 0 | 100 | 100 | `crates/od-ui/src/app/composer.rs:694-717`; e2e chat bước 8 |
| 47 | Studio-chat | Slash command (/mcp, /search) | 0 | 100 | 100 | `crates/od-ui/src/mention.rs` /mcp, /search; e2e chat bước 9 |
| 48 | Studio-chat | Inline model switcher (CLI/BYOK, agent, model) | 0 | 100 | 100 | `crates/od-ui/src/app/composer.rs:1172-1253` agent, model, reasoning; e2e chat bước 10 |
| 49 | Studio-chat | Queue: sửa, kéo thả, steer | 100 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:435-487` |
| 50 | Studio-chat | Quote to chat | 0 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:242` quote-selection; e2e chat bước 13c |
| 51 | Studio-chat | Trạng thái run (Working, Thinking) | 100 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:632` |
| 52 | Studio-chat | Todo list + plan pill "bước N/M" | 100 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:412` |
| 53 | Studio-chat | Dòng sinh media, audio player | 0 | 50 | 100 | `crates/od-ui/src/app/media_rows.rs:98-185` dòng sinh media (ô, tiến trình, lỗi, Retry), gắn ở `chat_view.rs:571`; audio player `chat_ops.rs:325`; e2e c2 bước 6; `shots/c2-media-row-light.png` |
| 54 | Studio-chat | "Files from this turn" (Open/Download) | 50 | 100 | 100 | `crates/od-ui/src/app/chat_ops.rs:285-333` Open, Download, +N; e2e chat bước 13 |
| 55 | Studio-chat | Copy tin nhắn | 0 | 100 | 100 | `crates/od-ui/src/app/chat_ops.rs:349` |
| 56 | Studio-chat | "Continue remaining tasks" | 0 | 100 | 100 | `crates/od-ui/src/app/chat_ops.rs:379-381`; e2e chat bước 12 |
| 57 | Studio-chat | Question form | 100 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:850-865` |
| 58 | Studio-chat | GenUI surface (Authorize, Accept all…) | 0 | 0 | 100 | `crates/od-ui/src/app/genui_view.rs:88-312` (Continue/Cancel, Authorize/Skip, form, choice, diff Accept all/Reject all/Partial, nhớ câu trả lời, Revoke), gắn ở `chat_view.rs:264`; e2e c4 bước 4–5; `shots/c4/chat-genui-light.png` |
| 59 | Studio-chat | Next-step actions, suggested next steps | 0 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:669` |
| 60 | Studio-chat | Design toolbox follow-up (Match, Icons, Motion, Polish…) | 0 | 100 | 100 | `crates/od-ui/src/app/composer.rs:1036-1050`, `chat_view.rs:713`; e2e chat bước 13b |
| 61 | Studio-chat | Design Jury (critique theater) | 50 | 50 | 100 | `crates/od-ui/src/app/agent_w3.rs:199,608-646` vòng/vai/điểm; Interrupt `:535`, Replay `:543` + tốc độ `:511`, `crates/od-ui/src/jury_replay.rs:46`; e2e c2 bước 7; `shots/c2-jury-replay-light.png` |
| 62 | Studio-chat | Lỗi run, reconnect, retry | 50 | 100 | 100 | `crates/od-ui/src/app/chat_ops.rs:405` Reconnect, Retry `chat_view.rs:196`; e2e chat bước 14 |
| 63 | Studio-chat | Xuất log/conversation | 100 | 100 | 100 | `crates/od-ui/src/app/agent_w3.rs:364-376` |
| 64 | Studio-chat | Memory của project (+ "Remember that…") | 100 | 100 | 100 | `crates/od-ui/src/app/agent_w3.rs:248-319` |
| 65 | Studio-chat | Side chat | 100 | 100 | 100 | `crates/od-ui/src/app/agent_w3.rs:876-918` |
| 66 | Studio-chat | Thu gọn/resize pane chat | 0 | 100 | 100 | `crates/od-ui/src/app/chat_view.rs:389` thu gọn, `chat_ops.rs:214` resize, `studio.rs:1120` Show chat; e2e chat bước 16 |
| 67 | Studio-chat | Cảnh báo deliverable sau run | 100 | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:1378` |
| 68 | Studio-chat | Thumbs feedback, share to community, support dialog, balance gate | N/A | N/A | N/A | BỎ (telemetry/share/community/billing) |
| 69 | Studio-preview | Preview HTML sandbox + live reload | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:1321-1334`; e2e studio bước 4–5 (reload 47 ms) |
| 70 | Studio-preview | Toggle Preview / Code | 0 | 50 | 100 | Toggle `crates/od-ui/src/app/studio.rs:1350-1360`; tô màu `studio.rs:1570`, `crates/od-ui/src/highlight.rs:84` (HTML+CSS+JS, CSS, JS, Markdown); e2e preview bước 2; `shots/c1/code-view-light.png` |
| 71 | Studio-preview | Chọn thiết bị Desktop/Tablet/Mobile | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:48-57` Desktop/Tablet/Phone |
| 72 | Studio-preview | Zoom in/out/reset | 0 | 100 | 100 | `crates/od-ui/src/app/studio.rs:1391-1397`, phím Cmd +/−/0 |
| 73 | Studio-preview | Reload | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:295` reload_preview |
| 74 | Studio-preview | Chụp màn hình vào clipboard | 0 | 100 | 100 | `crates/od-ui/src/app/capture.rs:140`; e2e files bước 10 |
| 75 | Studio-preview | Screenshot to chat | 50 | 100 | 100 | `crates/od-ui/src/app/capture.rs:143`; e2e files bước 10 |
| 76 | Studio-preview | Comment mode: ghim, gửi kèm lượt sau | 100 | 100 | 100 | `crates/od-ui/src/app/preview_comments.rs:109-197` |
| 77 | Studio-preview | Tab Comments, board, trạng thái anchor cũ/mất, đính ảnh vào comment | 50 | 50 | 100 | `crates/od-ui/src/app/comments_panel.rs:55-230` tab Comments (đếm, gửi kèm, resolve, xoá), anchor stale `:175`/lost `:178`, ảnh `:195,297`, `crates/od-ui/src/comment_board.rs`; e2e preview bước 4; `shots/c1/comments-tab-light.png` |
| 78 | Studio-preview | Draw/Mark (pen, box, text, undo/redo) | 50 | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:1282-1305` pen, box, text, undo, redo, clear; e2e files bước 13 |
| 79 | Studio-preview | Manual edit (layers, Content/Style/Attributes/Html, style controls, upload ảnh, xoá phần tử) | 50 | 50 | 100 | `crates/od-ui/src/app/manual_edit.rs:498-731` layers, tab Content/Style/Attributes/Html, upload ảnh `:604`, xoá phần tử `:545`, undo/redo `:536,540`; mở từ `preview_tools.rs:650`; e2e preview bước 3; `shots/c1/manual-edit-attributes-light.png` |
| 80 | Studio-preview | Inspect panel | 100 | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:717` |
| 81 | Studio-preview | Tweaks | 100 | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:911` |
| 82 | Studio-preview | Palette swap 5 preset | 100 | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:662` |
| 83 | Studio-preview | Lint anti-slop | 100 | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:625,942-969` |
| 84 | Studio-preview | Lịch sử phiên bản: xem, khôi phục | 100 | 100 | 100 | `crates/od-ui/src/app/versions.rs:92-149` |
| 85 | Studio-preview | Deck: điều hướng, rail thumbnail | 100 | 100 | 100 | `crates/od-ui/src/app/deck.rs:51-278` |
| 86 | Studio-preview | Speaker notes: xem, sửa, lưu | 50 | 100 | 100 | `crates/od-ui/src/app/deck.rs:357-397` sửa và lưu vào deck; e2e files bước 11 |
| 87 | Studio-preview | Present: trong tab / fullscreen / tab mới / presenter mode | 50 | 100 | 100 | `crates/od-ui/src/app/deck.rs:167-177` trong tab, fullscreen, tab mới, presenter (`presenter.rs:245-282`, đồng bộ); `(audit run) shots/b4/presenter-light.png` |
| 88 | Studio-preview | Markdown viewer/editor (Editor/Split/Preview) | 0 | 100 | 100 | `crates/od-ui/src/app/doc_view.rs:25-271` Editor/Split/Preview, auto-save; `(audit run) shots/b4/markdown-split-light.png` |
| 89 | Studio-preview | Live artifact viewer (Preview/Code/Data/Refresh history) | 0 | 100 | 100 | `crates/od-ui/src/app/live_view.rs:186-232` Preview/Code/Data/Refresh/History; `(audit run) shots/b4/live-artifact-light.png` |
| 90 | Studio-preview | Viewer ảnh | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:1304` |
| 91 | Studio-preview | Viewer PDF/Office/video/audio | 50 | 100 | 100 | `crates/od-ui/src/app/doc_view.rs:345-406` PDF, video, audio, Office có trích text, Download/Open |
| 92 | Studio-preview | Finalize design package (DESIGN.md) | 50 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs:343,387-418` Finalize/Re-finalize theo DESIGN.md; e2e files bước 12c |
| 93 | Studio-preview | Hand off: chọn editor, framework, copy prompt, Continue in CLI | 50 | 100 | 100 | `crates/od-ui/src/app/export_extra.rs:156-194` framework, target editor, prompt; Cmd+Shift+K `files_panel.rs:1379`; `(audit run) shots/b4/handoff-dialog-light.png` |
| 94 | Studio-preview | Brand ready prompt | 0 | 100 | 100 | `crates/od-ui/src/app/project_actions.rs:182-209` brand progress/ready/use/dismiss; `(audit run) shots/b4/brand-ready-light.png` |
| 95 | Studio-preview | Share link, visibility | N/A | N/A | N/A | Share/cloud: BỎ |
| 96 | Studio-files/tabs | Panel file (cây, breadcrumb) | 50 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:576-682` cây, thư mục thu gọn có đếm, breadcrumb |
| 97 | Studio-files/tabs | Upload, paste as text file, kéo thả | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:482-485,545` upload, paste as text, kéo thả; e2e files bước 2, 4 |
| 98 | Studio-files/tabs | New sketch | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:495`, `sketch.rs:168`; `(audit run) shots/b4/sketch-light.png` |
| 99 | Studio-files/tabs | New document (Markdown từ mẫu) | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:489` |
| 100 | Studio-files/tabs | Thư mục: tạo, chuyển | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:498`, move `:909` |
| 101 | Studio-files/tabs | Menu dòng: open in tab, download, copy path, delete | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:903-911` open, new tab, download, copy path, full path, rename, move, delete |
| 102 | Studio-files/tabs | Chọn nhiều: tải ZIP, xoá | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:804-809` ZIP, move, delete |
| 103 | Studio-files/tabs | Nhóm/lọc theo loại, phân trang | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:528,734-767` theo loại, lọc, show more; `(audit run) shots/b4/files-by-kind-light.png` |
| 104 | Studio-files/tabs | Tạo design system từ project | 0 | 100 | 100 | `crates/od-ui/src/app/project_actions.rs:141`; e2e files bước 12d |
| 105 | Studio-files/tabs | Thanh tab file, đóng tab | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:1145-1168` |
| 106 | Studio-files/tabs | Tab launcher "+" (mở file, tạo mới) | 50 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:1268-1287` mở file, document, page, terminal |
| 107 | Studio-files/tabs | Tab terminal | 100 | 100 | 100 | `crates/od-ui/src/app/terminal.rs:294` |
| 108 | Studio-files/tabs | Tab browser tích hợp | 0 | 0 | N/A | Browser tab: N/A (quyết định đã chốt) |
| 109 | Studio-files/tabs | Pages list, tạo page | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:361,492` nhóm Pages, tạo page |
| 110 | Studio-files/tabs | Focus workspace / Show chat | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:1329`, Show chat `studio.rs:1120`; `(audit run) shots/b4/focus-workspace-light.png` |
| 111 | Studio-files/tabs | Quick switcher Cmd/Ctrl+P | 100 | 100 | 100 | `crates/od-ui/src/app/switcher.rs:72` |
| 112 | Export | PDF | 100 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs:17-42` Pdf |
| 113 | Export | PPTX ảnh | 100 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs` Pptx |
| 114 | Export | PPTX editable | 0 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs:49` export-pptx-editable (qua Chrome cục bộ) |
| 115 | Export | ZIP project | 100 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs` Zip |
| 116 | Export | HTML standalone | 100 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs` Html |
| 117 | Export | Export Markdown (file) | 0 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs` Markdown; e2e files bước 12 |
| 118 | Export | Export ảnh có chọn định dạng | 50 | 100 | 100 | PNG, JPEG, WebP `crates/od-ui/src/app/export_menu.rs:21-23` |
| 119 | Export | Export JSX / preview as HTML | 0 | 0 | 50 | Export as JSX `crates/od-ui/src/app/export_menu.rs:29,127`, `crates/od-ui/src/jsx_export.rs:7-17`; e2e c2 bước 8. **Thiếu**: “Export React component as HTML” (app gốc `runtime/exports.ts:855`, menu `FileViewer.tsx:7044`) và preview render component JSX/TSX (`FileViewer.tsx:6910`); bản Rust mở `.jsx/.tsx` như code |
| 120 | Export | Save as template | 0 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs:329`, `export_extra.rs:74-100` |
| 121 | Export | MP4 HyperFrames | 50 | 100 | 100 | MP4 trong menu Export `crates/od-ui/src/app/export_menu.rs:26` + trang Media `media_page.rs:922` |
| 122 | Export | Open in new tab, copy path, download file | 0 | 100 | 100 | `crates/od-ui/src/app/export_menu.rs:317-325` |
| 123 | Export | Deploy Vercel/Cloudflare, social share, quick share | N/A | N/A | N/A | Deploy/share: BỎ |
| 124 | Design systems | Trang Design systems (Your systems, presets, lọc) | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:177-315`; e2e library PASS |
| 125 | Design systems | Chọn design system khi tạo project | 100 | 100 | 100 | `crates/od-ui/src/app/home.rs:353`, `new_project.rs:162` |
| 126 | Design systems | Tạo DS từ mô tả thương hiệu | 100 | 100 | 100 | `crates/od-ui/src/app/ds_create.rs:141-187` |
| 127 | Design systems | Tạo DS từ URL, GitHub, file, Figma, paste DESIGN.md, copy DS có sẵn | 0 | 100 | 100 | `crates/od-ui/src/app/ds_create.rs:519-584` website, GitHub, Figma qua agent, file, DESIGN.md/CSS, copy bộ có sẵn; e2e library fromWebsite, fromCss |
| 128 | Design systems | "Start from a brand" picker | 0 | 100 | 100 | `crates/od-ui/src/app/ds_create.rs:519,628` brand picker |
| 129 | Design systems | Tiến trình sinh theo pha | 50 | 100 | 100 | `crates/od-ui/src/app/ds_create.rs:569` 4 pha |
| 130 | Design systems | Review (Looks good / Needs work), revision accept/reject | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:917-947,1054-1075` review, revision nhận/bỏ |
| 131 | Design systems | Kit view (Showcase, Tokens, DESIGN.md), brand detail, preview modal | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:997-1211` showcase, tokens, DESIGN.md, fullscreen |
| 132 | Design systems | Sửa/copy/lưu DESIGN.md, upload font/logo | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:1009-1022,1156` sửa/chép DESIGN.md, upload asset |
| 133 | Design systems | Default for new chats | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:878` |
| 134 | Design systems | Download DS, Edit with agent | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:872,893` |
| 135 | Design systems | Import DS từ GitHub/shadcn trong Settings | 0 | 100 | 100 | Trong luồng Tạo của trang Design systems (GitHub qua agent, theme shadcn dán vào); không tải registry shadcn vì không ra mạng |
| 136 | Design systems | Rebuild token contract, điểm chất lượng | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:1142-1150` điểm, rebuild |
| 137 | Design systems | Publish, share/sync to team | N/A | N/A | N/A | Publish/team: BỎ |
| 138 | Plugins/Skills | Danh sách, chi tiết plugin | 50 | 100 | 100 | `crates/od-ui/src/app/plugins_page.rs:234-332,552` |
| 139 | Plugins/Skills | Dùng plugin (Use, Remix) | 50 | 100 | 100 | `crates/od-ui/src/app/plugins_page.rs:607-627`; e2e library pluginUse |
| 140 | Plugins/Skills | Form input plugin | 0 | 100 | 100 | `crates/od-ui/src/app/plugins_page.rs:616` InputsForm |
| 141 | Plugins/Skills | Thêm plugin (từ prompt, link, folder), install/uninstall | 0 | 100 | 100 | `crates/od-ui/src/app/plugins_page.rs:930-971` agent, link, folder; uninstall `:595` |
| 142 | Plugins/Skills | Registry sources, trust, marketplace | 0 | 0 | N/A | Marketplace/registry: N/A (quyết định đã chốt) |
| 143 | Plugins/Skills | Danh sách, chi tiết skill | 0 | 100 | 100 | `crates/od-ui/src/app/plugins_page.rs:251,705` |
| 144 | Plugins/Skills | Sửa skill (tạo, sửa, xoá, user override) | 0 | 100 | 100 | `crates/od-ui/src/app/plugins_page.rs:734-836` tạo, sửa (override), xoá |
| 145 | Plugins/Skills | Plugin rail trong composer | 0 | 100 | 100 | Plugin vào composer qua menu "+" và @ (`crates/od-ui/src/app/composer.rs:1091`), đúng hành vi app gốc (rail tắt, `showRail={false}`) |
| 146 | Plugins/Skills | Publish/contribute plugin, share with team | N/A | N/A | N/A | Publish/community/team: BỎ |
| 147 | Templates/Examples | Gallery có lọc (loại, surface, nhóm ngành) | 50 | 100 | 100 | `crates/od-ui/src/app/templates_page.rs:222-246` |
| 148 | Templates/Examples | Thẻ template có preview sống | 100 | 100 | 100 | `crates/od-ui/src/app/templates_page.rs:259,379`, `lazy_frame.rs` |
| 149 | Templates/Examples | Dùng template để tạo | 100 | 100 | 100 | `crates/od-ui/src/app/templates_page.rs:407`, Home `home.rs:975` |
| 150 | Templates/Examples | Preview modal (fullscreen, Remix, export) | 0 | 100 | 100 | `crates/od-ui/src/app/templates_page.rs:399-407` fullscreen, tab mới, tải, Remix, Use |
| 151 | Templates/Examples | Prompt template ảnh/video (Copy prompt) | 50 | 100 | 100 | `crates/od-ui/src/app/templates_page.rs:440-488` Copy, Generate |
| 152 | Templates/Examples | Template do user lưu | 0 | 100 | 100 | `crates/od-ui/src/app/templates_page.rs:541-601` |
| 153 | Templates/Examples | Community gallery, share template | N/A | N/A | N/A | Community/share: BỎ |
| 154 | Settings | Chọn agent + rescan | 100 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:222`; `(audit run) shots/manage/shots/settings-light.png` |
| 155 | Settings | Model, reasoning effort, service tier | 50 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:271,287,298` model, reasoning, service tier |
| 156 | Settings | Advanced: proxy, đường dẫn tuỳ chỉnh | 0 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:428-505` Advanced env (proxy, đường dẫn) |
| 157 | Settings | Test kết nối agent | 0 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:308`; e2e manage bước 3 |
| 158 | Settings | BYOK: preset provider, key, base URL, fetch models | 50 | 100 | 100 | `crates/od-ui/src/app/byok_form.rs:216-295` preset, protocol, base URL, key, fetch models, test, save; `(audit run) shots/manage/shots/settings-api-light.png` |
| 159 | Settings | Quyền agent (Automatic / Ask first) | 100 | 100 | 100 | Settings ▸ quyền agent (Automatic/Ask first) |
| 160 | Settings | Ngôn ngữ | 50 | 50 | 100 | 20 ngôn ngữ: en, vi + 18 locale của app gốc (`crates/od-ui/locales/*.txt`, tải khi cần, fallback en), RTL cho ar/fa; picker `settings_page.rs:363`; e2e c4 bước 1–2 (không tràn layout trên 12 màn × 19 ngôn ngữ); `shots/c4/home-ar-light.png` |
| 161 | Settings | Theme | 100 | 100 | 100 | Settings ▸ Appearance |
| 162 | Settings | Thông báo: âm thanh, desktop notification | 0 | 100 | 100 | `crates/od-ui/src/app/notify.rs:240-268` âm thanh, desktop, test |
| 163 | Settings | Project locations | 0 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:724-767` thêm, mặc định, scan |
| 164 | Settings | Global rules / instructions | 0 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:544-556` |
| 165 | Settings | Memory toàn cục (loại, hooks, import, MEMORY.md) | 50 | 50 | 100 | `crates/od-ui/src/app/memory_settings.rs:60-331` công tắc bật/tắt `:64`, hooks `:78`, import (text/file/scan) `:218-264`, sửa MEMORY.md `:310-331`, gắn ở `settings_page.rs:406`; e2e c2 bước 9; `shots/c2-settings-memory-light.png` |
| 166 | Settings | Media providers (key, base URL) | 50 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:847-888`; `(audit run) shots/manage/shots/settings-media-light.png` |
| 167 | Settings | About (version, export diagnostics cục bộ, reset onboarding) | 0 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:928-945` version, export diagnostics, reset onboarding |
| 168 | Settings | Menu cài đặt nhanh (avatar menu) | 0 | 0 | 100 | `crates/od-ui/src/app/quick_menu.rs:142-234` (theme, ngôn ngữ, section, Shortcuts, Docs, Report), gắn ở `mod.rs:471`; e2e c2 bước 10; `shots/c2-quick-menu-light.png` |
| 169 | Settings | Labs, privacy, account, wallet, update | N/A | N/A | N/A | BỎ (labs/privacy/account/wallet/update) |
| 170 | Agent runtimes | Claude Code (stream, resume, steer) | 100 | 100 | 100 | `crates/od-runtime`; e2e studio (mock claude) |
| 171 | Agent runtimes | Codex | 100 | 100 | 100 | `crates/od-runtime` |
| 172 | Agent runtimes | Cursor, OpenCode, Copilot, Qwen | 100 | 100 | 100 | `crates/od-runtime` |
| 173 | Agent runtimes | Họ ACP (Kimi, Hermes, Kiro, Kilo, Vibe, Trae, Reasonix, Devin, DeepSeek Harness) | 100 | 100 | 100 | `crates/od-runtime/src/parse/dsh.rs` và họ ACP |
| 174 | Agent runtimes | CLI khác (Aider, Amp, Pi, Qoder, MiMo, Codebuddy, AtomCode, Grok Build, Antigravity, DeepSeek TUI) | 0 | 0 | 100 | `crates/od-runtime/src/def.rs:450-548` grok-build, qoder, amp, pi, deepseek (TUI), aider, antigravity, codebuddy, mimo, atomcode; parser `crates/od-runtime/src/parse/{pi,qoder,antigravity}.rs`; e2e c3 bước 1 (phát hiện), bước 3 (một lượt chat qua Qoder) |
| 175 | Agent runtimes | Phát hiện agent + Refresh | 100 | 100 | 100 | `GET /api/agents`, `crates/od-ui/src/app/onboarding.rs:131` Refresh |
| 176 | Agent runtimes | Test kết nối | 0 | 100 | 100 | `crates/od-ui/src/app/settings_page.rs:308`, onboarding test; e2e manage bước 1 |
| 177 | Agent runtimes | BYOK preset (26 provider) | 50 | 100 | 100 | `crates/od-ui/src/byok.rs`; 25/26 preset tại `94dd674` (vLLM đã có `byok.rs:174`, còn thiếu Azure OpenAI vì cần protocol `azure` riêng). Giữ 100 như bản đợt B |
| 178 | Agent runtimes | AMR cloud | N/A | N/A | N/A | Cloud: BỎ |
| 179 | MCP | MCP server `od mcp` (stdio) | 100 | 100 | 100 | `crates/od-cli/src/mcp_cmd.rs`, `crates/od-mcp`; e2e studio bước 15d |
| 180 | MCP | Cài MCP vào agent (tab theo client, copy command) | 50 | 100 | 100 | `crates/od-ui/src/app/integrations.rs:241-302` snippet theo client, deeplink, Install/Uninstall; `(audit run) shots/manage/shots/integrations-agents-light.png` |
| 181 | MCP | MCP server ngoài (stdio/HTTP, args, test) | 100 | 100 | 100 | `crates/od-ui/src/app/mcp_settings.rs:78-249` |
| 182 | MCP | OAuth MCP | 100 | 100 | 100 | `crates/od-ui/src/app/mcp_settings.rs:246` Connect/Sign in |
| 183 | MCP | Catalog mẫu MCP | 0 | 100 | 100 | `crates/od-ui/src/app/integrations.rs:181-208` |
| 184 | MCP | Ghi chú forward tới agent nào | 0 | 100 | 100 | `crates/od-ui/src/app/integrations.rs:111-115` |
| 185 | MCP | Connectors (Composio) | 0 | N/A | N/A | Connectors Composio: N/A (quyết định đã chốt) |
| 186 | MCP | Use everywhere (hướng dẫn CLI + MCP) | 0 | 100 | 100 | `crates/od-ui/src/app/integrations.rs:333-350`; `(audit run) shots/manage/shots/integrations-everywhere-light.png` |
| 187 | Media | Provider media | 50 | 50 | 100 | `crates/od-media/src/provider.rs:70-84`: OpenAI, fal, ElevenLabs, Volcengine, Grok, Nano Banana, ImageRouter, OpenRouter, Custom Image, Leonardo, MiniMax, FishAudio, SenseAudio, AIHubMix, Tavily (+ HyperFrames cục bộ). Đủ mọi provider `integrated: true` của app gốc trừ `vela` (OpenDesign Cloud, cloud) và `stub`; 9 provider `integrated: false` của app gốc cũng chỉ là placeholder. e2e c3 bước 4–5; `shots/c3/shots/settings-media-providers-light.png` |
| 188 | Media | Project ảnh/video/audio trong UI (model, aspect, voice) | 0 | 100 | 100 | `crates/od-ui/src/app/media_page.rs:615-734` |
| 189 | Media | Audio player trong chat | 0 | 100 | 100 | Audio trong chat `crates/od-ui/src/app/chat_ops.rs:325`, thư viện `media_page.rs:437` |
| 190 | Media | Library (lọc, upload, Chat to design) | 0 | 100 | 100 | `crates/od-ui/src/app/media_page.rs:346-404` |
| 191 | Media | Web Clipper | 0 | 0 | N/A | Web Clipper: N/A (quyết định đã chốt) |
| 192 | Media | HyperFrames → MP4 | 50 | 100 | 100 | `crates/od-ui/src/app/media_page.rs:922-944` |
| 193 | Automations | Trang Automations | 100 | 100 | 100 | `crates/od-ui/src/app/automations.rs:483` |
| 194 | Automations | Tạo automation (tên, prompt, lịch, project) | 50 | 100 | 100 | `crates/od-ui/src/app/automations.rs:496-554,753-817` preset hourly/daily/weekly, timezone, tạo project mới mỗi lần |
| 195 | Automations | Thao tác dòng: Run now, bật/tắt, xoá, mở conversation, sửa | 50 | 100 | 100 | `crates/od-ui/src/app/automations.rs:920-931` bật/tắt, Run now, Edit, xoá |
| 196 | Automations | Lịch sử run | 100 | 100 | 100 | `crates/od-ui/src/app/automations.rs:948-963` |
| 197 | Automations | Crystallize, evolution proposals | 0 | 0 | 100 | `crates/od-ui/src/app/automations.rs:877-888` Crystallize một run, `:588-657` proposals Apply/Reject; e2e c4 bước 3; `shots/c4/automations-proposals-light.png` |
| 198 | Automations | Template automation | 0 | 100 | 100 | `crates/od-ui/src/app/automations.rs:114-158` 7 template của app gốc |
| 199 | Automations | Orbit (tóm tắt connector hằng ngày) | 0 | 50 | N/A | Orbit: N/A (quyết định đã chốt) |
| 200 | Automations | Refresh live artifact | 0 | 100 | 100 | `crates/od-ui/src/app/live_view.rs:199` Refresh + history; template live-artifact maintainer |
| 201 | Desktop shell | Cửa sổ chính, daemon trong process | 100 | 100 | 100 | `apps/desktop/src/main.rs:270` |
| 202 | Desktop shell | Menu App/Edit/View/Window | 100 | 100 | 100 | `apps/desktop/src/main.rs:198-206` Menu::default + Project |
| 203 | Desktop shell | Menu Help (Documentation, Report Issue) | 0 | 50 | 100 | `apps/desktop/src/main.rs:232-247` Help: Documentation, Report Issue, Show Data Folder, Shortcuts, Command palette; xử lý `:287-288` (guide đóng gói `docs_page.rs:45`, Report copy chẩn đoán, không ra mạng). Chưa chạy shell Tauri; bản web cùng hành động qua Quick menu, e2e c2 bước 10; `shots/c2-docs-light.png` |
| 204 | Desktop shell | Folder picker native | 100 | 100 | 100 | `apps/desktop/src/main.rs` pick_folder |
| 205 | Desktop shell | Mở thư mục trong Finder | 100 | 100 | 100 | Handoff ▸ Open project folder, Help ▸ Show Data Folder |
| 206 | Desktop shell | Export PDF/ảnh bằng engine trình duyệt | 100 | 100 | 100 | Chrome cục bộ qua CDP (decisions.md #2) |
| 207 | Desktop shell | Splash | 0 | 100 | 100 | `apps/desktop/static/index.html:6` splash khi daemon khởi động |
| 208 | Desktop shell | Đồng bộ theme native | 0 | 100 | 100 | `apps/desktop/src/main.rs:123-133` set_theme theo Appearance |
| 209 | Desktop shell | Cửa sổ pet | 0 | 0 | N/A | Pet: N/A (quyết định đã chốt) |
| 210 | Desktop shell | Auto-update, invite deep link, crash diagnostics, Develop menu AMR | N/A | N/A | N/A | auto-update N/A (quyết định); cloud/telemetry BỎ |
| 211 | Phím tắt | Cmd/Ctrl+, mở Settings | 0 | 100 | 100 | `crates/od-ui/src/app/mod.rs:278` |
| 212 | Phím tắt | Cmd/Ctrl+K tìm project | 0 | 100 | 100 | `crates/od-ui/src/app/mod.rs:266` (palette) |
| 213 | Phím tắt | Cmd/Ctrl+B thu gọn rail | 0 | 100 | 100 | `crates/od-ui/src/app/mod.rs:282-287`; trong Studio là ẩn/hiện panel file `studio.rs:840` |
| 214 | Phím tắt | Cmd/Ctrl+P quick switcher | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:836` |
| 215 | Phím tắt | Cmd/Ctrl+T tab mới | 50 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:1361` mở tab launcher |
| 216 | Phím tắt | Cmd/Ctrl+W đóng tab | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:856` |
| 217 | Phím tắt | Ctrl+Tab / Ctrl+Shift+Tab | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:876` |
| 218 | Phím tắt | Ctrl+PageUp/PageDown, Cmd+Opt+←/→ | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:1368-1375` |
| 219 | Phím tắt | Cmd/Ctrl+1…9 | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:864` |
| 220 | Phím tắt | Cmd/Ctrl+Shift+K Continue in CLI | 0 | 100 | 100 | `crates/od-ui/src/app/files_panel.rs:1379` |
| 221 | Phím tắt | Enter / Shift+Enter gửi, xuống dòng | 100 | 100 | 100 | `crates/od-ui/src/app/composer.rs`, `chat_view.rs` |
| 222 | Phím tắt | @ và / trong composer | 0 | 100 | 100 | `crates/od-ui/src/app/composer.rs:717`, `mention.rs` |
| 223 | Phím tắt | Cmd/Ctrl+Enter gửi comment | 50 | 100 | 100 | `crates/od-ui/src/app/preview_comments.rs:182` |
| 224 | Phím tắt | Cmd/Ctrl+Z trong draw | 0 | 100 | 100 | `crates/od-ui/src/app/preview_tools.rs:1211` |
| 225 | Phím tắt | Deck: →, ←, PageUp/Down, Home, End, R | 50 | 100 | 100 | `crates/od-ui/src/app/studio.rs:898-901`, R ở presenter `presenter.rs:211` |
| 226 | Phím tắt | Esc đóng modal, present, mode | 100 | 100 | 100 | `crates/od-ui/src/app/studio.rs:884` |
| 227 | Phím tắt | Phím trong DS kit (E/C/U/R/⌫) | 0 | 100 | 100 | `crates/od-ui/src/app/ds_page.rs:661-666` (E/C/U/R/⌫) |
