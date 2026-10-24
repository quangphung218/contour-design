# The Contour brand

**Contour**: design with your agent, on your machine.

Crate and binary names (`od-*`, `od`) and the `OD_*` environment variables stay as they are (docs/decisions.md §5). The display name is Contour: window, menus, sidebar, onboarding, CLI help, README. The OpenDesign name and logo never appear in the UI; credit to the original app lives in `NOTICE` and `README.md`.

## Meaning

A contour is the outline that defines a shape: the first line a designer draws. The mark is **one continuous contour line**. It traces the left and top edges of a frame (the artboard), runs down the right edge, then folds inward into a cursor whose tip points at the frame's corner. The frame is the thing being designed; the cursor is you and your agent working on it together.

## Name

- Always "Contour", with a capital C, in every UI string, doc and store listing. Never "CONTOUR" or "contour" as the product name.
- Never "OpenDesign" or "Open Design" in the UI or for this product.
- Internal names do not change: crates `od-*`, binary `od`. Lowercase `contour` appears only in identifiers: file names (`contour-*.svg`), the data folder, the bundle id `dev.contour.desktop` and the repo `contour-design`.

## Files

| File | Used for |
|---|---|
| `assets/brand/contour-mark.svg` | Accent-colour mark, light backgrounds |
| `assets/brand/contour-mark-dark.svg` | Mark for dark backgrounds |
| `assets/brand/contour-mark-mono.svg` | Monochrome, `currentColor` (print, engraving, system icons) |
| `assets/brand/contour-mark-small.svg` | Thicker stroke for 16–32 px |
| `assets/brand/contour-wordmark.svg` | Mark + the word "Contour" (outlined, no font needed), light backgrounds |
| `assets/brand/contour-wordmark-dark.svg` | Wordmark for dark backgrounds |
| `assets/brand/contour-wordmark-mono.svg` | Wordmark in `currentColor` |
| `assets/brand/contour-app-icon.svg`, `contour-app-icon-small.svg` | 1024 app icon: macOS-style squircle, 100 px safe margin (small: heavier stroke for 16–32 px renders) |
| `assets/brand/contour-favicon.svg` | Browser tab favicon (32 px rounded tile) |
| `assets/brand/png/contour-{mark,mark-dark,app-icon}-{16,32,48,64,128,256,512,1024}.png` | PNG renders of the mark, dark mark and app icon |
| `apps/desktop/icons/` | Tauri icon set: `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.png`, `icon.icns`, `icon.ico` (names fixed by `tauri.conf.json`) |
| `crates/od-ui/assets/contour-mark.svg` | Decorative mark in the UI sidebar and onboarding (no `<title>`) |
| `crates/od-ui/assets/favicon.svg`, `favicon-32.png` | UI favicon (copy of `contour-favicon.svg` and its 32 px render) |
| `assets/brand/src/` | Sources: `build.py`, `glyphs.py`, `contour-glyphs.json`, `render.mjs` |

## Regenerate

The mark's path is the `MARK` constant in `assets/brand/src/build.py`; edit it there, never hand-edit individual SVGs.

1. Wordmark outlines (only when the font or the name changes). The lettering is Geist SemiBold (SIL OFL 1.1, the same font as the UI), shaped with the font's own kerning plus -2% em tracking. Needs `fonttools`, `brotli` and `uharfbuzz`:

   ```bash
   python3 -m venv /tmp/brand-venv && /tmp/brand-venv/bin/pip install fonttools brotli uharfbuzz
   /tmp/brand-venv/bin/python assets/brand/src/glyphs.py crates/od-ui/vendor/fonts/Geist-Variable.woff2 Contour 600 > assets/brand/src/contour-glyphs.json
   ```

2. SVGs (standard library only):

   ```bash
   python3 assets/brand/src/build.py
   ```

3. PNGs, the Tauri icon set (`icon.icns` via `iconutil`, macOS only), `icon.ico` and the UI favicon. Needs Playwright from `crates/od-ui/e2e` (`npm ci` there first). Only needed when the mark or the app icon changes:

   ```bash
   (cd crates/od-ui/e2e && node ../../../assets/brand/src/render.mjs)
   ```

## Colour

The mark uses exactly the design system accent (`docs/design-system.md`); no new colours.

| Role | Light | Dark |
|---|---|---|
| Mark | `#2a7560` (`--color-accent`) | `#4fae8c` |
| Wordmark text | `#1c1b18` (`--color-text`) | `#edebe5` |
| App icon background | vertical gradient `#358a71` → `#1f5f4d`, white mark | same as light |

## Geometry and clear space

- 48 grid. Stroke width 3.6 (4.6 for the small version), round caps and round fold corners.
- Wordmark: cap height 24 grid units, baseline on the cursor's lowest point (y 36), letters start 18 units after the mark.
- Minimum clear space around the mark: **1/4 of the mark height** on each side. Wordmark: the height of the letter "C".
- Minimum size: mark 16 px (use the small version), wordmark 20 px tall.
- App icon: 824×824 body in a 1024 frame, 100 px margin for the shadow, mark fills about 50% of the body.

## Usage

- Sidebar and onboarding: monochrome mark on an accent tile (`brand_mark()` in `crates/od-ui/src/app/mod.rs`).
- Docs, README: the wordmark (`contour-wordmark.svg`); on dark backgrounds use the `-dark` version.
- Single-colour use (print, embossing): the `-mono` version.

## Don'ts

- Do not recolour the mark to anything other than the accent, and do not put a gradient on the mark (the gradient is only the app icon background).
- Do not rotate, distort, or add shadows or outlines to the mark.
- Do not split the frame and the cursor into two shapes; the point of the mark is one continuous contour line.
- Do not place the accent mark on an accent or busy background; use the white mono version.
- Do not retype the wordmark in a live font; use the outlined SVG.
- Do not use the OpenDesign name or logo for this product.
