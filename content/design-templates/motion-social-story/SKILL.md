---
name: motion-social-story
en_name: "Vertical social story"
en_description: "A ten-second 9:16 story in three cards: a bold opener, a product card with a price tag, and a call to action, with story progress bars."
description: |
  A 10 s vertical (9:16, 1080×1920) story for social feeds: three cards
  — a bold opener, a product card with a price sticker, and a call to
  action — joined by vertical push transitions, with story-style progress
  bars at the top. Replace copy, price and colours.
triggers:
  - "social story"
  - "vertical video"
  - "9:16"
  - "story video"
  - "reel template"
  - "shorts template"
od:
  mode: video
  surface: video
  platform: desktop
  scenario: motion
  category: motion-template
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  example_prompt: |
    10-second 9:16 story: "New drop" opener on tangerine, product card
    "Fold Tote — recycled canvas" with "€49" sticker, CTA "Tap the link to
    shop" on cream. Progress bars at the top.
---

# Vertical social story

A Contour motion template: a 10 s, 1080×1920 (9:16)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0.2–1.2 s | Kicker drops in; "New" and "drop." whip in from opposite sides. |
| 3.1–3.6 s | Push up to the product card; the card tilts in and the price sticker spins on. |
| 6.4–6.9 s | Push up to the call to action; the arrow bobs, the button pops. |
| 0–10 s | Three story progress bars fill, one per card. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 10s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 10.000%. To move a beat, change its
  percentage; to change the length, change `--film`, `data-duration` and
  the percentages together.
- `window.__timelines.main` is the seekable master timeline. Its `seek(t)`
  pins every CSS animation to `t`, so the render is the same every time.
  Keep it when you edit; add new motion as CSS animations, not timers.
- Sizes are in `cqw` (1cqw = 10.8px on the 1080px grid), so the stage
  scales to the preview and renders at exactly 1080×1920.
- System fonts and inline SVG only. Nothing is downloaded, no images, no
  web fonts, no CDN scripts.
- Sound: none. The film declares `od-motion-audio` `silent`. If the
  brief asks for sound, add a local `<audio>` track the motion-design skill
  describes, and say where it came from.

## Palette

| Token | Value |
|---|---|
| `tangerine` | #ff6b2c |
| `cream` | #fff4e6 |
| `ink` | #1c1410 |
| `muted` | #7a5b45 |

## What to change for a new brief

- Card 1: kicker `#k1`, words `#h1` / `#h2` (one short word each).
- Card 2: the tote drawing is inline SVG inside `#prod`; replace it with your product drawn as SVG, the price `#tag` and name `#pname`.
- Card 3: call to action `#cta` and button `#btn`. Keep platform names out unless the brief gives them.
- Progress bars `#g1`–`#g3` match the card timing: 0–3.3 s, 3.3–6.6 s, 6.6–10 s.

## Workflow

1. Scaffold a source folder: `"$OD_NODE_BIN" "$OD_BIN" media scaffold --project "$OD_PROJECT_ID" --composition-dir "motion-source/film"`.
2. Replace `motion-source/film/index.html` with this template's
   `example.html`, then change only what the brief asks: copy, colours,
   timing. Keep the composition root, `data-poster-time` inside the film
   and the master timeline.
3. Render: `"$OD_NODE_BIN" "$OD_BIN" media generate --project "$OD_PROJECT_ID" --surface video --model hyperframes-html --composition-dir "motion-source/film" --output "film.mp4"`.
   If it prints a task id, run `"$OD_NODE_BIN" "$OD_BIN" media wait <id>` until it exits 0.
4. Open the review frames it prints, then link the MP4 and the source HTML
   in the reply.
