---
name: motion-line-chart
en_name: "Animated line chart"
en_description: "A ten-second growth chart: a line draws across twelve months while the headline counts to +212%, a launch is pinned on the way and the last value pulses."
zh_name: "动态折线图"
zh_description: "10 秒增长折线图：折线沿十二个月绘出，标题数字同步涨到 +212%，途中标注一次发布，终点数值脉动。"
description: |
  A 10 s, 16:9 data film for growth updates and reports: a line chart draws
  across twelve months while the headline counts up in step, an event is
  pinned where it happened, the area fills in and the last value pulses.
  Put your series in the path, the axis labels and the headline.
triggers:
  - "animated line chart"
  - "growth chart video"
  - "data animation"
  - "metrics over time"
  - "chart for report video"
  - "动态折线图"
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
    10-second line chart on dark navy: monthly active members draw from 9.8k
    to 30.6k across 2026 while "+212%" counts up, August is pinned "Shared
    notebooks launched", then the end point pulses with "30.6k".
---

# Animated line chart

A Contour motion template: a 10 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`. Ledgerleaf and its numbers are made up.

## What is in the film

| Time | Beat |
|---|---|
| 0.2–1.2 s | Kicker and headline rise in. |
| 1.2–6.0 s | The line draws while the headline counts to +212%. |
| 4.1–5.0 s | The August point is pinned with a note. |
| 5.6–6.8 s | The area fills; the last point pops with its value. |
| 6.6–9.8 s | The end point pulses, then the chart fades for the loop. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 10s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 10%. To move a beat, change its
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
| `background` | #0f1222 |
| `ink` | #eef0ff |
| `line` | #5eead4 |
| `note` | #f472b6 |
| `muted` | #8a90b8 |

## What to change for a new brief

- The series: the `d` of `#line` and `#area` (viewBox units: x every 120 from 40, y = 560 − value / 40 × 520).
- Axis labels `.yl`/`.xl`, headline end value: `--n: 212` in the `#num` keyframes.
- Pinned note `#note` with `#pin`/`#ann`; end value `#val`.
- Line `#5eead4` and note `#f472b6` colours.

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
