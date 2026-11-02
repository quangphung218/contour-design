---
name: motion-bar-chart
en_name: "Animated bar chart"
en_description: "A ten-second data clip: six bars grow in turn with counting values, the best period lights up and a takeaway line lands."
description: |
  A 10 s animated bar chart for data stories and reports: axis draws,
  six bars grow in turn while their values count up, the highest bar is
  highlighted and a one-line takeaway lands. Replace labels, values (bar
  height = value × 6px) and the takeaway.
triggers:
  - "animated chart"
  - "bar chart animation"
  - "data animation"
  - "chart video"
  - "stats video"
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
    10-second animated bar chart: "Weekly sign-ups (thousands)", six
    periods Q1–Q6 with values 38, 52, 47, 71, 64, 88; highlight Q6 in teal,
    takeaway "Up 2.3× since Q1". Navy background.
---

# Animated bar chart

A Contour motion template: a 10 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0.2–1.4 s | Title and subtitle land; the baseline draws left to right. |
| 1.2–4.6 s | Six bars grow one after another; each value counts up with its bar. |
| 5.2 s | The takeaway pill pops in beside the highlighted bar. |
| 9.3 s | The chart fades out. |

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
| `background` | #0e1a2b |
| `ink` | #eaf1f8 |
| `bars` | #3d6a99 |
| `highlight` | #2ad4b0 |
| `muted` | #8fa6bf |

## What to change for a new brief

- Data: the six `.col` blocks. Set each bar's inline `height` to value × 6px and the matching `--n` end value in its counter keyframes (`#v1`–`#v6`).
- Title `#title`, subtitle `#sub` (keep "sample data" unless the numbers are real), takeaway pill `#take`.
- Highlight: the `.hi` class on one bar (teal `#2ad4b0`).

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
