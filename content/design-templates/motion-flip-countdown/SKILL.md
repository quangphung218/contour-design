---
name: motion-flip-countdown
en_name: "Flip clock countdown"
en_description: "A ten-second flip-clock countdown for doors opening: an hours, minutes and seconds board ticks down with a flap on every second, a bar drains, then an open line lands."
zh_name: "翻页时钟倒计时"
zh_description: "10 秒翻页时钟倒计时：时、分、秒面板逐秒翻动，进度条随之耗尽，最后弹出开场字样。"
description: |
  A 10 s, 16:9 flip-clock countdown for doors, drops and live streams: an
  hours : minutes : seconds board where the seconds card flips once per
  tick, a progress bar drains under it, then the board clears for a bold
  "We're open." line. Change the start second, the event line and colours.
triggers:
  - "flip clock countdown"
  - "split flap countdown"
  - "doors open countdown"
  - "live in seconds"
  - "event starting soon"
  - "翻页倒计时"
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
    10-second flip-clock countdown on deep navy: hours and minutes at 00, the
    seconds card flips from 08 to 00, an amber bar drains, then "We're open."
    punches in over the event line.
---

# Flip clock countdown

A Contour motion template: a 10 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`. Harbourlight Live is a made-up event.

## What is in the film

| Time | Beat |
|---|---|
| 0–0.7 s | Header and the 00 : 00 : 08 board rise in. |
| 1.15–7.3 s | The seconds flip down once every 0.85 s with a folding flap; the bar drains. |
| 7.5–8.0 s | The board shrinks away. |
| 7.9–9.7 s | "We're open." punches in, holds, then fades for the loop. |

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
| `background` | #0b1d2a |
| `card` | #17324a |
| `ink` | #fdfaf3 |
| `accent` | #ffb703 |
| `muted` | #8fb3c9 |

## What to change for a new brief

- The seconds: the `--n` stops in `#sec` (one per tick; leading zero comes from `decimal-leading-zero`).
- Hours and minutes are static text in the first two `.card`s.
- Header `#head`, event line `#event`, end line `#open`.
- Accent `#ffb703`.

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
