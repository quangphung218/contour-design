---
name: motion-countdown
en_name: "Countdown"
en_description: "A ten-second countdown from five: each number punches in inside a ring that empties with the second, then a launch line."
description: |
  A 10 s countdown for launches, streams and event openers: five big
  numbers punch in one per second inside a ring that empties with the
  second, then a "We're live" end line with a burst. Change the end line
  and colours; keep one number per second.
triggers:
  - "countdown"
  - "countdown timer"
  - "launch countdown"
  - "stream starting"
  - "5 4 3 2 1"
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
    10-second countdown 5 to 1 on deep violet, a ring empties each second,
    numbers punch in, then "We're live" with a burst of lines.
---

# Countdown

A Contour motion template: a 10 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0.5–5.5 s | 5, 4, 3, 2, 1: each number punches in from 1.8× and the ring empties over its second. |
| 5.4 s | The ring blows out. |
| 5.6–6.8 s | "We're live" pops in with a burst of eight lines. |
| 9.3 s | The end line fades. |

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
| `background` | #1a1033 |
| `ink` | #fdf8ff |
| `ring` | #ff5fa2 |
| `burst` | #ffd23f |
| `muted` | #b9a8d9 |

## What to change for a new brief

- End line `#live` and the caption `#sub`.
- Ring colour `#ff5fa2`, burst `#ffd23f`.
- Start from a higher number: add `#d6`… and shift each second's keyframes; the ring keyframes repeat once per second.

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
