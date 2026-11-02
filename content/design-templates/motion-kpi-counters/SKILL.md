---
name: motion-kpi-counters
en_name: "KPI counters"
en_description: "An eight-second metrics clip: three tiles count up to their numbers while sparklines draw and change badges slide in."
description: |
  An 8 s KPI clip for monthly updates and dashboards on video: three
  tiles count up to their numbers, sparklines draw on, change badges slide
  in. Replace the labels, end values (integers) and sparkline paths.
triggers:
  - "kpi animation"
  - "metrics video"
  - "counter animation"
  - "number animation"
  - "stats counter"
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
    8-second KPI clip on warm white: three tiles — "Active riders" 4820
    (+12%), "Avg. trip (min)" 18 (−3%), "Repeat rate" 73% (+8 pts) — numbers
    count up and sparklines draw.
---

# KPI counters

A Contour motion template: a 8 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0.2–0.9 s | Heading lands. |
| 0.6–1.9 s | Three tiles rise in, staggered by 0.25 s. |
| 0.9–3.6 s | Numbers count up; sparklines draw on. |
| 2.8–3.8 s | Change badges slide in. |
| 7.3 s | The board fades up and out. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 8s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 12.500%. To move a beat, change its
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
| `background` | #faf7f2 |
| `tile` | #ffffff |
| `ink` | #1d232b |
| `line` | #5b5bd6 |
| `up` | #17803d |
| `down` | #b4361f |

## What to change for a new brief

- Tiles: label `.k`, end value in the `#n1`–`#n3` counter keyframes (integers; add a suffix like `%` next to the span), badge `#d1`–`#d3` (class `down` turns it red).
- Sparklines: each `path` is drawn in a 240×80 box; keep `pathLength="100"`.
- Accent line colour: `#5b5bd6`.

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
