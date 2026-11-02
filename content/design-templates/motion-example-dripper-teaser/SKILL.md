---
name: motion-example-dripper-teaser
en_name: "Example: pour-over dripper teaser"
en_description: "A finished ten-second product teaser for a made-up ceramic coffee dripper: steam rises, features tick in, and a price end card lands."
description: |
  A finished 10 s product teaser for a made-up brand, Kettle & Kiln,
  and its ceramic pour-over dripper: the dripper is drawn in SVG, coffee
  drips and steam rises, three features tick in, and an end card with the
  price lands. Use it as a reference for a calm, warm product reveal.
triggers:
  - "product teaser"
  - "product reveal"
  - "coffee video"
  - "launch teaser"
  - "ecommerce video"
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
    10-second product teaser for "Kettle & Kiln" ceramic pour-over
    dripper: drawn dripper with dripping coffee and rising steam, line
    "Slow coffee, made simple.", features "Hand-glazed · 2-cup · Dishwasher
    safe", end card "Spring 2027 · $38". Warm espresso tones.
---

# Example: pour-over dripper teaser

A Contour motion example: a 10 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0.2–1.4 s | Warm glow; the dripper rises into place. |
| 0.8–8.0 s | Coffee drips and the cup fills; steam curls up on a loop. |
| 0.8–2.0 s | Brand line and headline land on the right. |
| 2.9–3.9 s | Three feature pills pop in. |
| 7.6–9.0 s | A cream end card wipes up with the product name and price. |

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
| `espresso` | #2a1d16 |
| `cream` | #f6ead8 |
| `caramel` | #e8a35c |
| `coffee` | #6b3a1f |
| `ceramic` | #e9dcc7 |

## What to change for a new brief

- Kettle & Kiln and the Kiln Dripper are made up. Replace the brand `#brand`, headline `#h`, features `#f1`–`#f3` and the end card `#eb` / `#ep`.
- The product is inline SVG in `#obj` (260×400 box); redraw it for another object, keep the steam paths `#st1`/`#st2` if it is hot.
- Coffee level `#cof` rises from its bottom edge; drops `#dr1`/`#dr2` fall on a loop.

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
