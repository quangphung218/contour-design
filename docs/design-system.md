# od-ui design system

The od-ui interface (Leptos CSR, `crates/od-ui`) uses its own design system, written in plain CSS with no framework. This document records the tokens, components and usage rules. Before changing the UI, read all of it before writing new CSS.

## Design direction

A desktop app for designers, refined in the Linear/Raycast vein: warm neutral backgrounds, a single green accent, text hierarchy through size + weight + colour, medium density, soft radii and shadows, complete hover/focus/active/disabled states, full dark mode, short motion.

Never use: purple gradients, emoji as icons, characters as icons (`↻ × ⤓ >_`), the em dash `—` in UI strings, fonts loaded from the network.

## Files

| File | Contents |
|---|---|
| `crates/od-ui/styles/tokens.css` | All tokens: `@font-face`, light/dark colours, type scale, spacing, radius, shadow, z-index, motion |
| `crates/od-ui/styles/components.css` | Reusable components (button, input, chip, card, tab, menu, dialog, toast, badge, tooltip, empty state, skeleton) |
| `crates/od-ui/styles/app.css` | App shell and each screen (Home, Projects, Studio, Settings…), composed only from tokens and components |
| `crates/od-ui/styles/library.css` | Library screens (Design systems, Plugins, Media, Templates), loaded after `app.css`, also composed only from tokens and components |
| `crates/od-ui/src/app/icons.rs` | `Glyph` + the `icon(glyph)` function |
| `crates/od-ui/vendor/remixicon/` | 109 Remix Icon 4.8.0 SVG icons, Apache-2.0 |
| `crates/od-ui/assets/` | Favicon and the Contour mark (generated from `assets/brand`, see [`brand.md`](brand.md)) |
| `crates/od-ui/vendor/fonts/` | Geist, Geist Mono (variable, woff2), SIL OFL 1.1 |

Load order in `index.html`: tokens → components → app → library. Components only read CSS variables and never hard-code colour values (exceptions: the white paper background of the preview and a few colours on the black background of presentation mode).

## Tokens

### Colour (roles, not colour names)

Light is the default. Dark follows the system (`prefers-color-scheme`) unless Settings pins a theme: `App` sets `data-theme="light|dark"` on `<html>`. The dark block is declared twice in `tokens.css` (system and manual choice); edit both together.

| Token | Used for | Light | Dark |
|---|---|---|---|
| `--color-canvas` | Window background, behind the sidebar | `#f2f1ed` | `#131210` |
| `--color-surface` | Main panel (`.main`), panes | `#fcfcfa` | `#1a1917` |
| `--color-surface-raised` | Card, input, menu, dialog | `#ffffff` | `#22211e` |
| `--color-surface-sunken` | Recessed areas: file tree, composer bar, code | `#f6f5f1` | `#161513` |
| `--color-fill` / `-strong` | Hover / pressed background of transparent items | `#eeece6` / `#e5e2da` | `#272622` / `#312f2b` |
| `--color-border` / `-subtle` / `-strong` | Normal border / divider inside a card / hover | | |
| `--color-text` / `-secondary` / `-tertiary` / `-disabled` | Four text levels | `#1c1b18` … | `#edebe5` … |
| `--color-accent` (+ `-hover`, `-pressed`) | Primary button, selected state, active icon | `#2a7560` | `#4fae8c` |
| `--color-on-accent` | Text on an accent background | `#ffffff` | `#0c1a14` |
| `--color-accent-soft` / `--color-accent-text` | Background + text of the selected thing (chip, tab, file, mode hint) | | |
| `--color-success` / `-warning` / `-danger` (+ `-soft`) | Status. Never used as decoration | | |
| `--color-inverse` / `--color-on-inverse` | Tooltip, toast | | |
| `--color-stage` | Background behind the preview, terminal pane | | |
| `--color-paper` | Preview iframe background (always white, like a real page) | | |
| `--color-terminal` | xterm background | | |
| `--color-cover-0..3`, `--color-cover-dot` | Four cover tones for template cards, pattern dots | | |
| `--color-focus-ring` | 3px focus ring | | |

One accent only. Neutrals come from a single warm-grey family; never mix in cool greys. Shadows are tinted warm brown (`rgb(60 46 24 / …)`), never pure black.

### Type

Fonts: `--font-sans` = Geist, `--font-mono` = Geist Mono, both bundled (`/fonts/`) with system font fallbacks. No Google Fonts.

| Token | px | Used for |
|---|---|---|
| `--text-2xs` | 10.5 | badges, numbers on thumbnails |
| `--text-xs` | 11.5 | meta, paths, captions |
| `--text-sm` | 12.5 | small buttons, field labels, dense lists |
| `--text-base` | 13.5 | default text |
| `--text-md` | 15 | h2, brief |
| `--text-lg` | 17 | dialog titles |
| `--text-xl` | 21 | |
| `--text-2xl` | 28 | page h1 |
| `--text-3xl` | 36 | Home title |

Weights: 400 body text, 500 buttons and labels, 600 headings. Large headings use `--tracking-tight`. Figures use `font-variant-numeric: tabular-nums`.

### Spacing, sizing, radius

- 4px grid: `--space-1` (4) … `--space-16` (64), plus `-0-5`, `-1-5`, `-2-5`, `-3-5` where a half step is needed.
- Control heights: `--control-xs` 22, `--control-sm` 28, `--control-md` 32, `--control-lg` 40. Icons: 14 / 16 / 20.
- Radius: tight inside, soft outside. `--radius-xs` 4 (badge, kbd), `-sm` 6 (small button, menu item), `-md` 8 (button, input), `-lg` 12 (card, main panel), `-xl` 16 (dialog, composer), `-full` (chip, send button).

### Shadow, layers, motion

- Shadows: `--shadow-xs` (button, input), `-sm` (active tab, panel), `-md` (card on hover, composer), `-lg` (menu, dialog, toast). `--shadow-accent` for the primary button.
- Layers: `--z-raised` 10, `--z-sticky` 20, `--z-popover` 40, `--z-present` 50, `--z-overlay` 60, `--z-modal` 70, `--z-toast` 80, `--z-tooltip` 90. Never write raw z-index numbers.
- Motion: `--duration-instant` 80ms, `-fast` 140ms, `-base` 200ms, `-slow` 280ms; easing `--ease-out`, `--ease-in-out`, `--ease-emphasized`. Animate only `transform`, `opacity`, colour and shadow. `prefers-reduced-motion: reduce` sets every duration to 0 and stops spinners.

## Components

| Class | Variants | Notes |
|---|---|---|
| `.btn` | `primary`, `ghost`, `danger`, `ghost danger`, `subtle-btn`, `active` / `aria-pressed="true"`; sizes `small`, `large`; `round` | Icon goes before the label: `{icon(Glyph::X)}{t(..)}`. Variants change the `--btn-bg`, `--btn-fg`, `--btn-border`, `--btn-bg-hover` variables |
| `.icon-btn` | `small`, `danger`, `active` / `aria-expanded` | `aria-label` is required; usually paired with `data-tip` |
| `.input`, `select`, `textarea.input` | `small` | Focus: accent border + `--color-focus-ring` ring. `select` has an arrow drawn with a gradient |
| `.field` | | `<label class="field"><span>Label</span><input …><span class="muted">hint</span></label>` |
| `.switch` | | `<label class="switch"><input type="checkbox"><span>…</span></label>` |
| `.radio-row` | | Selectable card with a description; accent border when `:checked` |
| `.chip` | `active` / `aria-checked="true"` | Picking an output type, form answers, attached comment chips |
| `.segmented` + `.seg` | `small`, `icons` | One choice among a few values (session mode, device, theme, library screen tabs). Selected: `.active`, `aria-checked`, `aria-pressed` or `aria-selected` |
| `.badge` | `accent`, `success`, `warning`, `danger`, `outline` | Small square corners (`--radius-xs`), never a pill |
| `.card` | `padded`, `interactive` | Use only when elevation signals hierarchy; otherwise use whitespace |
| `.tabs` + `.tab` | `active` | `.tab-name` (with icon) + `.tab-close` (shown on hover or when active) |
| `.menu-wrap` + `.menu` / `.popover` | `left` | `.menu-item` has an icon, `.menu-sep` is a group heading |
| `.overlay` + `.dialog` | `overlay.center` | `.dialog-head` (`.dialog-icon`), `.dialog-body`, `.dialog-foot`; `role="dialog"`, `aria-modal`, Esc and backdrop click to close. Use the `lib_ui::Modal` component: the backdrop is a `.modal-backdrop` sibling of the dialog (it does not wrap the dialog), because a button inside that closes the modal removes the DOM tree while the click is still bubbling |
| `lib_ui::ConfirmBar` | | Inline delete confirmation on the page (`--color-danger-soft` background). Never use `window.confirm`: the blocking dialog makes app handlers re-enter while they are still running |
| `.toast-region` + `.toast` | `error` | Export/handoff results, auto-hide after 5 seconds |
| `[data-tip]` | `data-tip-pos="right|left|top"` | CSS tooltip, shown after 350ms. Do not also put `title` on the same element |
| `.empty-state` | `compact` | `.empty-icon` + `<strong>` + `<p>` + an action button if any |
| `.skeleton` | | Loading placeholder shaped like the content (card, agent, template) |
| `.spinner` | `on-accent` | Only for work that is really running (run, export, design system creation) |
| `kbd` | | Keyboard shortcuts |

## Icons

- One set only: Remix Icon, "line" style (plus `stop-fill`, `checkbox-circle-fill`). Do not mix sets or hand-draw paths.
- Usage: `use super::icons::{Glyph, icon};` then `{icon(Glyph::History)}`. SVGs are `include_str!`-ed into the bundle, painted with `currentColor`, sized by the `.ico` class (16px, or `ico-sm` 14 / `ico-lg` 20).
- Icons are always `aria-hidden`; the screen-reader name lives on the button (`aria-label` or visible text).
- Adding an icon: `npm pack remixicon@4.8.0`, copy the SVG into `vendor/remixicon/icons/`, add one line to `glyphs!` in `icons.rs`. Do not upgrade to 4.9+ (license change).

## Rules

1. Colour, font size, spacing, radius, shadow, z-index, motion duration: tokens only.
2. Every clickable element has all four states: hover, focus-visible (`--color-focus-ring` ring), active (slight press `scale(0.985)` or a darker background), disabled (50% opacity, `cursor: not-allowed`).
3. Icon-only buttons must have `aria-label`. Toggled-on things use `aria-pressed` / `aria-checked` / `aria-current`, and CSS reads those same attributes.
4. UI strings go through `t()` / `tf()` in `src/i18n.rs`, with both en + vi. No `—` in UI strings.
5. One accent. Success/warning/danger colours only for real status.
6. Check both light and dark before finishing: `crates/od-ui/e2e/screens.mjs` captures every main screen at 1440x900 in both modes.
7. When changing markup, keep every `data-testid`; e2e (`crates/od-ui/e2e/run.sh`) depends on them.

## App shell

- Left sidebar: Contour logo, the "Tìm hoặc chạy lệnh" (Search or run a command) box (opens the command palette), Home, Projects, Templates, Design systems, Plugins, Media, Automations, Integrations, Messages, then Keyboard shortcuts and Settings at the bottom. It collapses to a 56px rail; the choice is stored in `localStorage` separately for Studio (`od.sidebar.studio-collapsed`, collapsed by default) and other pages (`od.sidebar.collapsed`). Windows narrower than 960px become a rail automatically.
- Content sits in the `.main` panel floating over the canvas background.
- macOS desktop: the Tauri window uses an overlay title bar (`TitleBarStyle::Overlay`, title hidden). The UI detects Tauri + macOS, sets `data-shell="mac"` on `<html>` and reserves a 38px-high `.titlebar` strip as the window drag region (`data-tauri-drag-region`, permission `core:window:allow-start-dragging`).

## Studio

- Three areas: chat on the left, preview in the middle (the main area), file list on the right. The file column collapses (button at the top of the column, `Cmd/Ctrl+B`, stored in `od.studio.files-open`).
- Tab row: file tabs, Side chat, "+", then on the right History (icon) and Export. Export is the primary button (`primary`) while a file is open: it is the goal of the main flow.
- The preview toolbar is **a single row of icons with tooltips** (`data-tip`, `aria-label`), left to right: reload, `Preview | Code`, device, file path (shrinkable), tool group (Add comment, All comments with a count, Inspect, Edit text, Edit element, Draw, Screenshot, Tweaks, Palette, Lint), zoom. Palette is a transparent `select` laid over its icon. Hints and status for the open tool sit in a strip below the toolbar (`ToolHints`).
- Narrow widths (`@container preview`): below 860px the `Preview | Code` labels and the Draw bar text are hidden; below 760px the tool panels and History float over the preview instead of squeezing it; below 560px the toolbar wraps. Windows below 1100px open Studio with the file column collapsed (until the user opens it, stored in `od.studio.files-open`).
- The Draw bar sits at the bottom of the page and does not cover the title; it wraps when narrow.
- The code view of the version being viewed shows that version's source. Turning on comments switches back to Preview. "Thử lại" (Retry) resends the original message's attachments too (stored per message, even after history reloads).
- Zoom is `transform: scale` on a frame laid out at device size (`zoom_box` + `zoomed` in `studio.rs`), so WebKit and Chromium match and comment pins land in the right place.
- Each agent turn has a status line `[data-testid=run-state]` (`data-phase` = `starting | working | done | failed | stopped`; when following a plan it shows "Bước N/M" (Step N/M)). The last turn, once no run is active, has a "Tiếp theo" (Next) row (`next-steps`): view the file just written, comment on the page, mobile version, another variation; on error, "Thử lại" (Retry) and "Mở Cài đặt" (Open Settings). Logic lives in `src/guide.rs`, tested natively.
- File column (`app/files_panel.rs`, pure logic in `src/file_tree.rs`): collapsible folder tree (click to expand, double-click to enter a folder, breadcrumb to leave), file search, view by type (chip filters, "Hiện thêm" (Show more) every 50 files), multi-select (ZIP, move, delete), row menu (open, new tab, download, copy path, rename, move, delete), "+" button (upload, paste as file, new document/page/sketch/folder), "…" menu (refresh, create design system from project, duplicate). Drop files onto the column to upload. The editor writes files with `PUT /content/*path` (exact name, no sanitizing).
- Menus opened inside scroll areas (file tree, tab bar, deck bar) use `position: fixed` computed from the button (`files_panel::anchor_style`) plus a `.menu-scrim` layer so clicking outside closes them; ordinary menus stay `absolute`.
- Non-HTML files get their own view instead of the iframe: Markdown (`doc_view.rs`, Soạn/Chia đôi/Xem trước (Editor/Split/Preview), autosave after 800 ms), sketch `*.sketch.json` (`sketch.rs`, shares strokes and undo with Draw), PDF/video/audio (the browser's viewer), Word/PowerPoint/Excel (text extracted with `od-export::office`). These views are not rebuilt when the preview reloads.
- Deck: Present has a menu (in tab, fullscreen, new tab, presenter view). Presenter view is the `/present/:id` route opened in its own window, syncing slides with Studio over `BroadcastChannel` (`presenter.rs`; a slide requested by the other side is not re-broadcast, `deck::should_announce`).
- If the project cannot load (bad id, daemon down), the error is pinned at the top of the chat pane (`load-error`) with a Retry button, and no sample prompts are shown. Other Studio errors appear as a red toast at bottom centre (`data-testid=notice`), auto-hiding after 8 seconds. HTTP errors start with a plain-language sentence based on the status code (`status_hint` in `api.rs`), followed by the daemon's original message.

## Composer (Home and Studio)

- Shared `app/composer.rs`: `MentionArea` (input + `@`/`/` palette, file paste and drag-and-drop), `PlusMenu` ("+": attach file, Plugins, Skills, MCP, design toolbox, page-specific items via `children`), `ContextChips`, `UploadTray`, `ModelPicker`. Pure logic in `src/mention.rs`, `src/models.rs`, `src/toolbox.rs`, `src/intent.rs`.
- Menus and popovers open upward in chat (`.menu.up`, `.popover.up`); they close on outside click or Esc (`close_outside`).
- `@` palette: groups Design files, Plugins, Skills, MCP (`.menu-sep`), selected row `aria-selected="true"`. Attachments: chips with a 24 px thumbnail for images, `data-state` = `waiting | uploading | failed`.
- The chat pane is resizable (`.chat-resizer`, 16 px arrows), collapsible from a button on the conversation bar, width stored in `od.studio.chat-width`. Components that remove themselves inside their own handler (rename form, delete dialog) change state on the next task.

## Command palette and keyboard shortcuts

- `Cmd/Ctrl+K` opens the command palette (`app/palette.rs`): commands (new project, pages, toggle theme, shortcuts), projects, and in Studio also Studio commands and files. Fuzzy search via `fuzzy::score`. `Cmd/Ctrl+P` remains the file-only quick switcher.
- `?` (outside text inputs) opens the shortcuts sheet (`app/shortcuts.rs`). When adding a shortcut, also add it to `GROUPS` there.
- `Cmd/Ctrl+,` opens Settings; `Cmd/Ctrl+B` outside Studio collapses the sidebar (in Studio it hides/shows the file column). In Studio: `Cmd/Ctrl+T` opens the tab launcher, `Ctrl+PageUp/PageDown` and `Cmd/Ctrl+Alt+←/→` switch tabs, `Cmd/Ctrl+Shift+K` copies the handoff prompt, `R` returns to the first slide, `Cmd/Ctrl+Z` / `Cmd/Ctrl+Shift+Z` undo/redo strokes (`files_panel::on_studio_key`).
- Desktop app: the Help menu (Keyboard shortcuts, Command palette, Open data folder) and Settings… send `od:menu` to the page; the app theme also sets the native window theme (`desktop::set_theme`); the window opens on the splash `apps/desktop/static/index.html` while the daemon starts; files dropped onto the window reach the page as HTML5 drops.
- Dialogs opened from the shell close on the next task (`TimeoutFuture::new(0)`), not directly in the click handler: closing immediately drops a listener that the same click is still passing through, and wasm reports `closure invoked recursively or after being dropped`.
