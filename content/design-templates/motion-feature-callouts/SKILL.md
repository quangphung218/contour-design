---
name: motion-feature-callouts
en_name: "Product feature callouts"
en_description: "A ten-second product explainer: a phone mockup turns into view and three callout lines draw out to feature labels."
description: |
  A 10 s feature callout film for product explainers: a CSS-drawn
  phone turns into view, its screen builds, and three callout lines draw
  out to feature labels one after another. Replace the screen content,
  labels and accent.
triggers:
  - "feature callouts"
  - "product explainer"
  - "app feature video"
  - "product video"
  - "ui animation"
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
    10-second product callout video: a phone mockup of a budgeting app
    turns into view; callouts "Split bills in one tap", "Smart monthly
    caps", "Works offline" draw out. Soft sage background.
---

# Product feature callouts

A Contour motion template: a 10 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0.2–1.4 s | The phone turns into view from a 40° angle. |
| 1.0–2.6 s | The screen builds: balance counts up, the cap bar fills, rows slide in. |
| 2.8–7.0 s | Three callouts, 1.4 s apart: dot, line draws, label slides in. |
| 9.3 s | Everything fades. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 10s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 10.000%. To move a beat, change its
  percentage; to change the length, change `--film`, `data-duration` and
  the percentages together.
- `window.__timelines.main` is the seekable master timeline. Its `seek(t)`
  pins every CSS animation to `t`, so the render is the same every time.
  Keep it when you edit; add new motion as CSS animations, not timers.
- Sizes are in `cqw` (1cqw = 19.2px on the 1920px grid), so the stage
  scales to the preview and renders at exactly 1920×1080.
- System fonts and inline SVG only. Nothing is downloaded, no images, no
  web fonts, no CDN scripts.
- Sound: none. The film declares `od-motion-audio` `silent`. If the
  brief asks for sound, add a local `<audio>` track the motion-design skill
  describes, and say where it came from.

## Palette

| Token | Value |
|---|---|
| `background` | #e5ece4 |
| `device` | #1e2a24 |
| `screen` | #fbfdfb |
| `green` | #3fa66b |
| `amber` | #f2a541 |

## What to change for a new brief

- Labels `#l1`–`#l3` (title + `small` line). Their positions are inline `left`/`top`; each line `#p1`–`#p3` ends next to its label and starts on the phone (SVG drawn on the 1920×1080 grid).
- Screen content inside `.screen`: the balance counter `#amt` (end value in its `--n` keyframes), the cap bar `#cap` and rows `#w1`–`#w3`.
- Accents: `#3fa66b` (green), `#f2a541` (amber).

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
