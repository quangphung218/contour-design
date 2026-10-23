# Thương hiệu Nét

**Nét**: thiết kế cùng agent, ngay trên máy (en: *Design with your agent, on your machine*).

Tên repo, crate và binary (`open-design-rs`, `od-*`, `od`) giữ nguyên. Chỉ tên hiển thị đổi: cửa sổ, menu, sidebar, onboarding, CLI help, README. Tên và logo OpenDesign không xuất hiện trong UI; ghi công cho app gốc nằm ở `NOTICE` và `README.md`.

## Ý nghĩa

"Nét" là nét vẽ: một đường liền, không nhấc bút. Mark là **một nét duy nhất** vẽ cạnh trái và cạnh trên của một khung (artboard), đi xuống cạnh phải rồi gập vào trong thành con trỏ chuột, mũi nhọn chỉ về góc khung. Khung là thứ được thiết kế, con trỏ là agent và người dùng cùng làm trên đó.

## File

| File | Dùng cho |
|---|---|
| `assets/brand/net-mark.svg` | Mark màu accent, nền sáng |
| `assets/brand/net-mark-dark.svg` | Mark cho nền tối |
| `assets/brand/net-mark-mono.svg` | Đơn sắc, `currentColor` (in, khắc, icon hệ thống) |
| `assets/brand/net-mark-small.svg` | Nét dày hơn cho 16–32 px |
| `assets/brand/net-wordmark{,-dark,-mono}.svg` | Mark + chữ "Nét" (đã outline, không cần font) |
| `assets/brand/net-app-icon.svg` | Icon app 1024: squircle kiểu macOS, lề an toàn 100 px |
| `assets/brand/net-favicon.svg` | Favicon tab trình duyệt (ô bo góc 32 px) |
| `assets/brand/png/` | PNG 16, 32, 48, 64, 128, 256, 512, 1024 của mark, mark tối và icon app |
| `apps/desktop/icons/` | Bộ icon Tauri: `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.png`, `icon.icns`, `icon.ico` |
| `crates/od-ui/assets/` | Favicon và mark trong sidebar/onboarding của UI |

Sinh lại toàn bộ từ một nguồn:

```bash
python3 assets/brand/src/build.py                          # SVG từ path trong build.py
(cd crates/od-ui/e2e && node ../../../assets/brand/src/render.mjs)   # PNG, icns (iconutil), ico, favicon
```

Path của mark nằm ở hằng `MARK` trong `build.py`; sửa ở đó, không sửa tay từng SVG. Chữ của wordmark là Geist SemiBold (SIL OFL 1.1, cùng font của UI), outline sẵn trong `src/net-glyphs.json` bằng `src/glyphs.py` (cần `fonttools`, `brotli`).

## Màu

Mark dùng đúng accent của design system (`docs/design-system.md`), không thêm màu mới.

| Vai trò | Light | Dark |
|---|---|---|
| Mark | `#2a7560` (`--color-accent`) | `#4fae8c` |
| Chữ wordmark | `#1c1b18` (`--color-text`) | `#edebe5` |
| Nền icon app | gradient dọc `#358a71` → `#1f5f4d`, mark trắng | như light |

## Hình học và khoảng trống

- Lưới 48. Nét dày 3.6 (bản small 4.6), đầu nét và góc gập bo tròn.
- Khoảng trống tối thiểu quanh mark: **1/4 chiều cao mark** mỗi phía. Wordmark: bằng chiều cao chữ "N".
- Cỡ nhỏ nhất: mark 16 px (dùng bản small), wordmark cao 20 px.
- Icon app: thân 824×824 trong khung 1024, lề 100 px cho bóng đổ, mark chiếm khoảng 50% thân.

## Cách dùng

- Sidebar và onboarding: mark đơn sắc trên ô accent (`brand_mark()` trong `crates/od-ui/src/app/mod.rs`).
- Tài liệu, README: wordmark (`net-wordmark.svg`), nền tối dùng bản `-dark`.
- Một màu duy nhất (in, dập nổi): bản `-mono`.

## Không nên

- Không đổi màu mark sang màu khác accent, không thêm gradient lên mark (gradient chỉ là nền icon app).
- Không xoay, kéo méo, đổ bóng hay viền thêm cho mark.
- Không tách khung và con trỏ thành hai hình; ý của mark là một nét liền.
- Không đặt mark accent lên nền accent hoặc nền nhiều chi tiết; dùng bản mono trắng.
- Không viết tên thành "NET", "Net" hay "nét" trong UI; luôn là "Nét".
- Không dùng tên hoặc logo OpenDesign cho sản phẩm này.
