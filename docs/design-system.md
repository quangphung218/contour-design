# Design system của od-ui

Giao diện od-ui (Leptos CSR, `crates/od-ui`) dùng một design system riêng, viết bằng CSS thuần, không framework. Tài liệu này ghi token, component và quy tắc dùng. Sửa giao diện thì đọc hết trước khi viết CSS mới.

## Hướng thiết kế

App desktop cho người làm thiết kế, tinh tế kiểu Linear/Raycast: nền neutral ấm, một màu accent xanh lá, chữ phân cấp bằng cỡ + độ đậm + màu, mật độ vừa, bo góc và đổ bóng nhẹ, đủ trạng thái hover/focus/active/disabled, dark mode đầy đủ, chuyển động ngắn.

Không dùng: gradient tím, emoji làm icon, ký tự làm icon (`↻ × ⤓ >_`), dấu gạch dài `—` trong chuỗi UI, font tải từ mạng.

## File

| File | Nội dung |
|---|---|
| `crates/od-ui/styles/tokens.css` | Mọi token: `@font-face`, màu light/dark, type scale, spacing, radius, shadow, z-index, motion |
| `crates/od-ui/styles/components.css` | Component dùng lại (button, input, chip, card, tab, menu, dialog, toast, badge, tooltip, empty state, skeleton) |
| `crates/od-ui/styles/app.css` | App shell và từng màn (Home, Projects, Studio, Settings…), chỉ ghép token và component |
| `crates/od-ui/styles/library.css` | Các màn thư viện (Design systems, Plugins, Media, Templates), nạp sau `app.css`, cũng chỉ ghép token và component |
| `crates/od-ui/src/app/icons.rs` | `Glyph` + hàm `icon(glyph)` |
| `crates/od-ui/vendor/remixicon/` | 109 icon SVG Remix Icon 4.8.0, Apache-2.0 |
| `crates/od-ui/assets/` | Favicon và mark Nét (sinh từ `assets/brand`, xem [`brand.md`](brand.md)) |
| `crates/od-ui/vendor/fonts/` | Geist, Geist Mono (variable, woff2), SIL OFL 1.1 |

Thứ tự nạp trong `index.html`: tokens → components → app → library. Component chỉ đọc biến CSS, không viết mã màu trực tiếp (ngoại lệ: nền giấy trắng của preview và vài màu trên nền đen của chế độ trình chiếu).

## Token

### Màu (vai trò, không phải tên màu)

Light là mặc định. Dark theo hệ thống (`prefers-color-scheme`), trừ khi Settings chọn cố định: `App` đặt `data-theme="light|dark"` trên `<html>`. Khối dark khai báo hai lần trong `tokens.css` (theo hệ thống và chọn tay), sửa thì sửa cả hai.

| Token | Dùng cho | Light | Dark |
|---|---|---|---|
| `--color-canvas` | Nền cửa sổ, sau sidebar | `#f2f1ed` | `#131210` |
| `--color-surface` | Panel chính (`.main`), pane | `#fcfcfa` | `#1a1917` |
| `--color-surface-raised` | Card, input, menu, dialog | `#ffffff` | `#22211e` |
| `--color-surface-sunken` | Vùng lõm: cây file, thanh composer, code | `#f6f5f1` | `#161513` |
| `--color-fill` / `-strong` | Nền hover / pressed của item trong suốt | `#eeece6` / `#e5e2da` | `#272622` / `#312f2b` |
| `--color-border` / `-subtle` / `-strong` | Viền thường / phân cách trong card / hover | | |
| `--color-text` / `-secondary` / `-tertiary` / `-disabled` | Bốn mức chữ | `#1c1b18` … | `#edebe5` … |
| `--color-accent` (+ `-hover`, `-pressed`) | Nút chính, trạng thái đang chọn, icon active | `#2a7560` | `#4fae8c` |
| `--color-on-accent` | Chữ trên nền accent | `#ffffff` | `#0c1a14` |
| `--color-accent-soft` / `--color-accent-text` | Nền + chữ của thứ đang chọn (chip, tab, file, mode hint) | | |
| `--color-success` / `-warning` / `-danger` (+ `-soft`) | Trạng thái. Không dùng làm màu trang trí | | |
| `--color-inverse` / `--color-on-inverse` | Tooltip, toast | | |
| `--color-stage` | Nền sau preview, terminal pane | | |
| `--color-paper` | Nền iframe preview (luôn trắng như trang thật) | | |
| `--color-terminal` | Nền xterm | | |
| `--color-cover-0..3`, `--color-cover-dot` | Bốn tông nền bìa card template, chấm hoạ tiết | | |
| `--color-focus-ring` | Vòng focus 3px | | |

Một accent duy nhất. Neutral cùng một họ xám ấm, không trộn xám lạnh. Bóng đổ nhuốm nâu ấm (`rgb(60 46 24 / …)`), không dùng đen thuần.

### Chữ

Font: `--font-sans` = Geist, `--font-mono` = Geist Mono, đều đóng gói trong bundle (`/fonts/`), có fallback font hệ thống. Không tải Google Fonts.

| Token | px | Dùng cho |
|---|---|---|
| `--text-2xs` | 10.5 | badge, số trên thumbnail |
| `--text-xs` | 11.5 | meta, đường dẫn, chú thích |
| `--text-sm` | 12.5 | nút nhỏ, label field, danh sách dày |
| `--text-base` | 13.5 | chữ mặc định |
| `--text-md` | 15 | h2, brief |
| `--text-lg` | 17 | tiêu đề dialog |
| `--text-xl` | 21 | |
| `--text-2xl` | 28 | h1 trang |
| `--text-3xl` | 36 | tiêu đề Home |

Độ đậm: 400 chữ thường, 500 nút và label, 600 tiêu đề. Tiêu đề lớn dùng `--tracking-tight`. Số liệu dùng `font-variant-numeric: tabular-nums`.

### Khoảng cách, kích thước, bo góc

- Lưới 4px: `--space-1` (4) … `--space-16` (64), thêm `-0-5`, `-1-5`, `-2-5`, `-3-5` cho chỗ cần chỉnh nửa bước.
- Chiều cao control: `--control-xs` 22, `--control-sm` 28, `--control-md` 32, `--control-lg` 40. Icon: 14 / 16 / 20.
- Bo góc: trong chặt, ngoài mềm. `--radius-xs` 4 (badge, kbd), `-sm` 6 (nút nhỏ, item menu), `-md` 8 (nút, input), `-lg` 12 (card, panel chính), `-xl` 16 (dialog, composer), `-full` (chip, nút gửi).

### Bóng đổ, lớp, chuyển động

- Bóng: `--shadow-xs` (nút, input), `-sm` (tab active, panel), `-md` (card khi hover, composer), `-lg` (menu, dialog, toast). `--shadow-accent` cho nút chính.
- Lớp: `--z-raised` 10, `--z-sticky` 20, `--z-popover` 40, `--z-present` 50, `--z-overlay` 60, `--z-modal` 70, `--z-toast` 80, `--z-tooltip` 90. Không viết số z-index trực tiếp.
- Chuyển động: `--duration-instant` 80ms, `-fast` 140ms, `-base` 200ms, `-slow` 280ms; easing `--ease-out`, `--ease-in-out`, `--ease-emphasized`. Chỉ animate `transform`, `opacity`, màu và bóng. `prefers-reduced-motion: reduce` đưa mọi duration về 0 và tắt spinner quay.

## Component

| Class | Biến thể | Ghi chú |
|---|---|---|
| `.btn` | `primary`, `ghost`, `danger`, `ghost danger`, `subtle-btn`, `active` / `aria-pressed="true"`; cỡ `small`, `large`; `round` | Icon đặt trước chữ: `{icon(Glyph::X)}{t(..)}`. Biến thể đổi biến `--btn-bg`, `--btn-fg`, `--btn-border`, `--btn-bg-hover` |
| `.icon-btn` | `small`, `danger`, `active` / `aria-expanded` | Bắt buộc có `aria-label`; thường kèm `data-tip` |
| `.input`, `select`, `textarea.input` | `small` | Focus: viền accent + vòng `--color-focus-ring`. `select` có mũi tên vẽ bằng gradient |
| `.field` | | `<label class="field"><span>Label</span><input …><span class="muted">gợi ý</span></label>` |
| `.switch` | | `<label class="switch"><input type="checkbox"><span>…</span></label>` |
| `.radio-row` | | Thẻ chọn có mô tả; viền accent khi `:checked` |
| `.chip` | `active` / `aria-checked="true"` | Chọn loại đầu ra, câu trả lời form, chip bình luận đính kèm |
| `.segmented` + `.seg` | `small`, `icons` | Một lựa chọn trong vài giá trị (chế độ phiên, thiết bị, theme, tab của màn thư viện). Đang chọn: `.active`, `aria-checked`, `aria-pressed` hoặc `aria-selected` |
| `.badge` | `accent`, `success`, `warning`, `danger`, `outline` | Vuông góc nhỏ (`--radius-xs`), không dùng viên thuốc |
| `.card` | `padded`, `interactive` | Chỉ dùng khi độ nổi nói lên thứ bậc; còn lại dùng khoảng trắng |
| `.tabs` + `.tab` | `active` | `.tab-name` (có icon) + `.tab-close` (hiện khi hover hoặc active) |
| `.menu-wrap` + `.menu` / `.popover` | `left` | `.menu-item` có icon, `.menu-sep` là tiêu đề nhóm |
| `.overlay` + `.dialog` | `overlay.center` | `.dialog-head` (`.dialog-icon`), `.dialog-body`, `.dialog-foot`; `role="dialog"`, `aria-modal`, Esc và bấm nền để đóng. Dùng component `lib_ui::Modal`: nền là `.modal-backdrop` đứng cạnh dialog (không bọc dialog), vì nút bên trong đóng modal sẽ gỡ cây DOM khi click còn đang nổi bọt |
| `lib_ui::ConfirmBar` | | Dòng xác nhận xoá ngay trên trang (nền `--color-danger-soft`). Không dùng `window.confirm`: hộp thoại chặn làm handler của app bị gọi lại khi đang chạy |
| `.toast-region` + `.toast` | `error` | Kết quả export/handoff, tự ẩn sau 5 giây |
| `[data-tip]` | `data-tip-pos="right|left|top"` | Tooltip CSS, hiện sau 350ms. Không dùng thêm `title` trên cùng phần tử |
| `.empty-state` | `compact` | `.empty-icon` + `<strong>` + `<p>` + nút hành động nếu có |
| `.skeleton` | | Khối chờ tải đúng hình dạng nội dung (card, agent, template) |
| `.spinner` | `on-accent` | Chỉ cho việc đang chạy thật (run, export, tạo design system) |
| `kbd` | | Phím tắt |

## Icon

- Bộ duy nhất: Remix Icon, kiểu "line" (thêm `stop-fill`, `checkbox-circle-fill`). Không trộn bộ khác, không tự vẽ path.
- Dùng: `use super::icons::{Glyph, icon};` rồi `{icon(Glyph::History)}`. SVG được `include_str!` vào bundle; tô bằng `currentColor`, cỡ theo class `.ico` (16px, hoặc `ico-sm` 14 / `ico-lg` 20).
- Icon luôn `aria-hidden`; tên cho trình đọc màn hình nằm ở nút (`aria-label` hoặc chữ hiện).
- Thêm icon: `npm pack remixicon@4.8.0`, chép SVG vào `vendor/remixicon/icons/`, thêm một dòng trong `glyphs!` ở `icons.rs`. Không nâng lên 4.9+ (đổi license).

## Quy tắc

1. Màu, cỡ chữ, khoảng cách, bo góc, bóng, z-index, thời gian chuyển động: chỉ lấy từ token.
2. Mọi phần tử bấm được có đủ bốn trạng thái: hover, focus-visible (vòng `--color-focus-ring`), active (lún nhẹ `scale(0.985)` hoặc nền đậm hơn), disabled (mờ 50%, `cursor: not-allowed`).
3. Nút chỉ có icon phải có `aria-label`. Thứ đang bật dùng `aria-pressed` / `aria-checked` / `aria-current`, CSS đọc chính các thuộc tính đó.
4. Chuỗi UI qua `t()` / `tf()` trong `src/i18n.rs`, đủ en + vi. Không đặt `—` trong chuỗi UI.
5. Một accent. Màu success/warning/danger chỉ cho trạng thái thật.
6. Kiểm tra cả light và dark trước khi xong: `crates/od-ui/e2e/screens.mjs` chụp mọi màn chính ở 1440x900 cho cả hai chế độ.
7. Đổi markup thì giữ nguyên mọi `data-testid`; e2e (`crates/od-ui/e2e/run.sh`) dựa vào chúng.

## App shell

- Sidebar trái: logo Nét, ô "Tìm hoặc chạy lệnh" (mở command palette), Home, Projects, Templates, Design systems, Plugins, Media, Automations, Integrations, Messages, rồi Phím tắt và Settings ở dưới cùng. Nút thu gọn thành rail 56px; lựa chọn lưu trong `localStorage` riêng cho Studio (`od.sidebar.studio-collapsed`, mặc định thu gọn) và các trang khác (`od.sidebar.collapsed`). Cửa sổ hẹp hơn 960px tự thành rail.
- Nội dung nằm trong panel `.main` nổi trên nền canvas.
- Desktop macOS: cửa sổ Tauri dùng title bar overlay (`TitleBarStyle::Overlay`, ẩn tiêu đề). UI phát hiện Tauri + macOS, đặt `data-shell="mac"` trên `<html>` và chừa dải `.titlebar` cao 38px làm vùng kéo cửa sổ (`data-tauri-drag-region`, quyền `core:window:allow-start-dragging`).

## Studio

- Ba vùng: chat bên trái, preview ở giữa (vùng chính), danh sách file bên phải. File thu gọn được (nút trên đầu cột file, `Cmd/Ctrl+B`, lưu ở `od.studio.files-open`).
- Toolbar preview, trái sang phải: tải lại, đường dẫn file, `Preview | Code`, thiết bị (Desktop/Tablet/Phone), zoom (`- 100% +`), History, Export. Hàng dưới là nhóm chú thích và soi phần tử: Comment, Inspect, Edit text, Draw, Tweaks, Palette, Lint. Cửa sổ hẹp (container dưới 660px) thì ẩn chữ, giữ icon.
- Code view của version đang xem là source của version đó. Bật bình luận thì tự về Preview. "Thử lại" gửi lại cả file đính kèm của tin gốc (lưu theo tin, cả khi tải lại lịch sử).
- Zoom là `transform: scale` trên khung đã layout ở kích thước thiết bị (`zoom_box` + `zoomed` trong `studio.rs`), để WebKit và Chromium giống nhau và pin comment theo đúng chỗ.
- Mỗi lượt của agent có một dòng trạng thái `[data-testid=run-state]` (`data-phase` = `starting | working | done | failed | stopped`; đang chạy theo plan thì hiện "Bước N/M"). Lượt cuối, khi không còn run nào, có hàng "Tiếp theo" (`next-steps`): xem file vừa ghi, bình luận lên trang, bản mobile, phương án khác; lỗi thì "Thử lại" và "Mở Cài đặt". Logic nằm ở `src/guide.rs`, test native.
- Không tải được project (id sai, daemon tắt) thì lỗi nằm cố định đầu khung chat (`load-error`), kèm nút Thử lại; không hiện prompt mẫu. Các lỗi khác của Studio hiện thành toast đỏ ở giữa đáy (`data-testid=notice`), tự ẩn sau 8 giây. Lỗi HTTP mở đầu bằng câu dễ hiểu theo mã trạng thái (`status_hint` trong `api.rs`), sau đó là thông điệp gốc của daemon.

## Command palette và phím tắt

- `Cmd/Ctrl+K` mở command palette (`app/palette.rs`): lệnh (project mới, các trang, đổi theme, phím tắt), project, và khi đang ở Studio thì thêm lệnh Studio và file. Tìm mờ bằng `fuzzy::score`. `Cmd/Ctrl+P` vẫn là quick switcher chỉ cho file.
- `?` (ngoài ô nhập) mở bảng phím tắt (`app/shortcuts.rs`). Thêm phím tắt mới thì thêm cả vào `GROUPS` ở đó.
- Hộp thoại mở từ shell đóng ở task kế tiếp (`TimeoutFuture::new(0)`), không đóng ngay trong handler click: đóng ngay làm rơi listener mà chính cú click đó còn đi qua, wasm báo `closure invoked recursively or after being dropped`.
