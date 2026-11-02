---
name: motion-example-bike-share-year
en_name: "Example: bike share year in numbers"
en_description: "A finished twelve-second data story for a made-up city bike scheme: trip counts tick up, a monthly line draws and the busiest month is called out."
description: |
  A finished 12 s year-in-numbers film for a made-up city bike scheme,
  Riverside Bikes: three headline numbers count up, a monthly ride line
  draws across the year, the busiest month gets a callout and a thank-you
  end line lands. Use it as a reference for a data-led recap.
triggers:
  - "year in review"
  - "year in numbers"
  - "data story"
  - "annual recap"
  - "infographic video"
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
    12-second year-in-numbers film for "Riverside Bikes 2026": 412k
    trips, 1,850 bikes, 96 stations count up; a line of monthly rides draws
    with July called out; end line "Thanks for riding". Teal and white.
---

# Example: bike share year in numbers

A Contour motion example: a 12 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0.2–1.0 s | Title lands. |
| 0.8–3.8 s | Three stat columns rise; 412k trips, 1,850 bikes and 96 stations count up. |
| 3.2–6.0 s | The monthly ride line draws across the year. |
| 5.0–6.4 s | A pin marks July and the callout slides in. |
| 9.6–10.3 s | A yellow iris opens on "Thanks for riding". |

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
| `teal` | #0b3d3a |
| `ink` | #f2fbf9 |
| `mint` | #7fd8c9 |
| `line` | #ffd166 |
| `muted` | #a9d9d1 |

## What to change for a new brief

- Riverside Bikes and its figures are made up. Replace the title `#ttl`, the three counters `#n1`–`#n3` (end values in their `--n` keyframes) and their labels.
- Monthly line `#ln`: 12 points on a 1000×340 box, x = 40 + month × 80, y = 300 − value × 4; move the pin `#pin` to the peak.
- Callout `#call` and end line `#end`.

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
