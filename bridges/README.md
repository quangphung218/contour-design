# Preview bridge

`od-bridge.js` is injected by `od-preview` at the top of every bridged preview
document (before any author script). It is written from scratch for
open-design-rs; nothing is copied from the original app.

The host and the bridge talk with `window.postMessage`. Every `type` starts
with `od:`. The bridge only accepts messages whose `source` is its parent
window, so a page cannot drive itself through the protocol.

## Host → bridge

| type | fields | effect |
|---|---|---|
| `od:slide` | `action`: `next` `prev` `first` `last` `go`; `index` (0-based, for `go`); `protocolVersion?` | Moves the deck, then replies `od:slide-state` |
| `od:slide-state-request` | | Replies `od:slide-state` |
| `od:select-mode` | `enabled` | Turns hover/click selection on or off |
| `od:snapshot` | `requestId`; `fullPage?`; `format?`: `svg` (default) or `png` | Replies `od:snapshot-result` |
| `od:size-request` | | Replies `od:size` |
| `od:locate` | `items`: `[{id, selector}]` (≤ 500) | Replies `od:located`, and again after every scroll or resize until the next `od:locate` |
| `od:inspect` | `selector` | Replies `od:inspect-result` |
| `od:inspect-set` | `selector`, `prop`, `value` | Overrides one inline style property, then replies `od:inspect-result` |
| `od:inspect-reset` | `selector?`, `prop?` | Undoes overrides of one property, one element, or (no `selector`) all |
| `od:inspect-replay` | `items`: `[{selector, prop, value}]` | Sets overrides again (the host replays its unsaved ones after a reload) |
| `od:inspect-extract` | | Replies `od:inspect-overrides` |
| `od:edit-mode` | `enabled` | Text-edit mode on or off |
| `od:palette` | `id`: `coral` `electric` `acid-forest` `risograph` `mono-noir`, or `null` | Recolors the page, or restores it; replies `od:palette-state` |
| `od:tweaks-request` | | Replies `od:tweaks` |
| `od:tweak-set` | `id`, `value` | Sets a declared tweak |

## Bridge → host

| type | fields | when |
|---|---|---|
| `od:ready` | `protocolVersion`, `capabilities` | Each document load (DOMContentLoaded) |
| `od:deck-ready` | `protocolVersion`, `capabilities`, `count` | Slides found |
| `od:slide-state` | `protocolVersion`, `active` (0-based), `count` | After a command, and whenever the visible slide changes |
| `od:esc` | | Escape pressed inside the preview |
| `od:hover`, `od:select` | `odId`, `selector`, `tag`, `text`, `rect` | Selection mode |
| `od:snapshot-result` | `requestId`, `width`, `height`, `format`, `dataUrl` (or `error`, `rasterError`) | After `od:snapshot` |
| `od:size` | `width`, `height` | Content size changed |
| `od:located` | `items`: `[{id, rect}]` (`rect` null when the selector matches nothing) | After `od:locate`, scroll, resize |
| `od:redirect-loop-blocked` | `hops` | The page reloaded itself more than 15 times, each < 1 s apart |
| `od:inspect-result` | `selector`, `tag`, `text`, `rect`, `styles` (computed), `overrides`, `textOnly`; or `missing: true` | After `od:inspect`, `od:inspect-set`, `od:inspect-reset` |
| `od:inspect-overrides` | `items`: `[{selector, prop, value}]`, `css` | After `od:inspect-extract` |
| `od:text-edit` | `selector`, `odId`, `tag`, `before`, `text` | A text edit was committed (Enter or blur) and changed the text |
| `od:palette-state` | `id` (or `null`) | After `od:palette` |
| `od:tweaks` | `tweaks`: the valid declared tweaks (may be empty) | Each document load, and after `od:tweaks-request` |

## How each part works

- **Deck**: slides are top-level matches of `[data-od-slide], .slide, .reveal .slides > section`. The active slide is the one marked `active`/`is-active`/`current`/`present`/`aria-current`/`data-active`, else the one covering most of the viewport. Navigation presses keys the deck already understands (Home, End, arrows), one step at a time; a deck that ignores keys is scrolled to the target slide.
- **Selection**: an element is named by its nearest `data-od-id`; without one, by a `tag:nth-of-type()` path from the nearest tagged ancestor or `html`.
- **Locate**: `querySelector` on each stored selector, `getBoundingClientRect` in viewport pixels, so the host can draw comment pins where the element is now.
- **Inspect**: computed style of a fixed property list. An override is an inline `style.setProperty`; the bridge keeps the inline value it replaced, so a reset restores the element exactly. The host writes overrides back to the source with `POST /api/projects/:id/source-edits`.
- **Text edit**: only text-only elements (no child elements; not `script`, `style`, form fields) become `contenteditable="plaintext-only"` on click. Enter or leaving the element commits, Esc restores. The host writes the text back with `expectText` = the text before, so a source that moved on is refused (409), never overwritten.
- **Palette**: each chromatic color (computed `color`, `background-color`, `border-color`, `fill`, `stroke`, plus custom properties declared on `:root`/`html`/`body`) takes the preset's hue and keeps its lightness, and its saturation above a floor; grays, near-black and near-white stay. `mono-noir` drops saturation. Every change is recorded and undone on the next swap.
- **Tweaks**: the artifact declares its knobs as JSON in a `script[type="application/json"][data-od-tweaks]` element: `{"tweaks": [{"id", "type", "label?", "var" | "attr", "default?", …}]}`. `type` is `color`, `range` (`min`, `max`, `step`, `unit?`), `select` (`options`: strings or `{label, value}`), `toggle` (`on?`, `off?`, default `"true"`/`"false"`) or `text`. A value lands on the root custom property `var` (`--name`) or the root attribute `attr` (`data-*`), and the bridge fires `od:tweak` (`detail: {id, value}`) on `window` for script-driven knobs. At most 24 tweaks.
- **Snapshot**: clones the DOM with computed styles inlined (scripts and bridge UI dropped) and wraps it in an SVG `foreignObject`. No screenshot API.
- **Redirect guard**: a hop counter in `window.name` survives reloads; past the limit the bridge calls `window.stop()` before any author script runs.
- **Web Storage shim**: opaque-origin sandboxes throw on `localStorage`/`sessionStorage`; the bridge swaps in an in-memory store.

The script must never contain the closing script tag sequence, since it is inlined (a unit test checks this).
