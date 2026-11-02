---
name: motion-example-weekend-forecast
en_name: "Example: weekend weather forecast"
en_description: "A finished eight-second square weather card for Lisbon: the sun turns, clouds drift, today's temperature counts up and three day cards flip in."
description: |
  A finished 8 s square (1:1) weather card, Weekend forecast for
  Lisbon (sample figures): the sun turns, clouds drift across, today's
  temperature counts up and three day cards flip in. Use it as a reference
  for recurring info cards — weather, scores, prices — on social feeds.
triggers:
  - "weather video"
  - "forecast card"
  - "info card"
  - "square video"
  - "daily update video"
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
    8-second square weather card "Weekend in Lisbon": turning sun,
    drifting clouds, 27° counting up, "Sunny, light breeze", three day
    cards Sat 27°/18°, Sun 24°/17°, Mon 19°/14°. Sky blue.
---

# Example: weekend weather forecast

A Contour motion example: a 8 s, 1080×1080 (1:1)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0–8 s | The sun turns slowly; two clouds drift in opposite directions. |
| 0.3–1.7 s | City line, today's temperature counting 12° → 27°, condition. |
| 2.0–3.3 s | Saturday, Sunday and Monday cards flip up in turn. |
| 7.4 s | The card fades out. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 8s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 12.500%. To move a beat, change its
  percentage; to change the length, change `--film`, `data-duration` and
  the percentages together.
- `window.__timelines.main` is the seekable master timeline. Its `seek(t)`
  pins every CSS animation to `t`, so the render is the same every time.
  Keep it when you edit; add new motion as CSS animations, not timers.
- Sizes are in `cqw` (1cqw = 10.8px on the 1080px grid), so the stage
  scales to the preview and renders at exactly 1080×1080.
- System fonts and inline SVG only. Nothing is downloaded, no images, no
  web fonts, no CDN scripts.
- Sound: none. The film declares `od-motion-audio` `silent`. If the
  brief asks for sound, add a local `<audio>` track the motion-design skill
  describes, and say where it came from.

## Palette

| Token | Value |
|---|---|
| `sky` | #cfe6ff |
| `ink` | #10233d |
| `sun` | #ffb627 |
| `rain` | #4f8fe8 |
| `muted` | #3c5a80 |

## What to change for a new brief

- The figures are samples, not a real forecast. Replace the city `#city`, today's temperature (end value in the `#n` `--n` keyframes), condition `#cond` and the three day cards `#d1`–`#d3`.
- Icons are inline SVG on a 60×60 box: sun, cloud, rain; copy the one you need.
- Sky `#cfe6ff`, sun `#ffb627`, ink `#10233d`.

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
