---
name: motion-example-design-week-opener
en_name: "Example: design week opener"
en_description: "A finished twelve-second event opener for a made-up design festival: a 3-2-1 build, a bold letter grid and the dates and venue."
description: |
  A finished 12 s opener for a made-up event, Northwind Design Week
  2027: a 3-2-1 count on colour fields, the event name built letter by
  letter on a bold grid, then dates and venue slide in under it. Use it as
  a reference for festival, conference and season openers.
triggers:
  - "event opener"
  - "conference opener"
  - "festival intro"
  - "event promo"
  - "save the date video"
od:
  mode: video
  surface: video
  platform: desktop
  scenario: motion
  category: motion-example
  preview:
    type: html
    entry: example.html
  design_system:
    requires: false
  example_prompt: |
    12-second opener for "Northwind Design Week 2027": 3-2-1 on red,
    blue and yellow fields, the name builds letter by letter on a black
    grid, then "14–18 June · Halden Hall" and "Talks · Studios · Night
    market".
---

# Example: design week opener

A Contour motion example: a 12 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0–3.4 s | 3 on red, 2 on blue, 1 on yellow, each field wiping over the last; the 1 closes as an iris. |
| 3.2–4.6 s | A grid fades in; NORTHWIND rises letter by letter; three blocks pop into the grid. |
| 4.5–6.3 s | "Design Week", dates and venue, and the programme line slide in. |
| 6–9.5 s | The blue circle slides one cell and back. |
| 11.3 s | Fade out. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 12s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 8.333%. To move a beat, change its
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
| `black` | #111111 |
| `paper` | #f5f2ea |
| `red` | #e63b2e |
| `blue` | #2b59c3 |
| `yellow` | #f7c331 |

## What to change for a new brief

- Northwind Design Week and Halden Hall are made up. Replace the name letters `#L1`… (one span per letter), `#nm2`, the edition line `#yr` and the details `#m1` / `#m2`.
- Count fields `#f3`, `#f2`, `#f1` wipe in from alternating sides; colours red `#e63b2e`, blue `#2b59c3`, yellow `#f7c331`.
- Grid blocks `#q1`–`#q3` are decoration; recolour or drop them.

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
