---
name: motion-logo-reveal
en_name: "Logo reveal"
en_description: "A six-second logo sting: a geometric mark draws itself, the wordmark rises letter by letter and a light sweep crosses it."
description: |
  A 6 s logo reveal sting for openers, outros and social bumpers: a
  stroke-drawn geometric mark, a letter-by-letter wordmark, a tagline and a
  light sweep. Swap the mark's SVG paths, the wordmark letters and the two
  accent colours.
triggers:
  - "logo reveal"
  - "logo animation"
  - "logo sting"
  - "brand reveal"
  - "logo intro"
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
    A 6-second logo reveal for "Lumen & Co": the mark draws itself in
    amber, the wordmark rises letter by letter, a light sweep crosses it,
    tagline "Light for small rooms". Dark ink background.
---

# Logo reveal

A Contour motion template: a 6 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0.2–1.7 s | The circle, triangle and square draw on in turn; the dot pops in. |
| 1.8–2.5 s | The wordmark rises letter by letter. |
| 2.6–3.6 s | A rule opens, the tagline fades up, a light sweep crosses the wordmark. |
| 5.3–5.9 s | Everything eases out to the dark background. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 6s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 16.667%. To move a beat, change its
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
| `background` | #0f1115 |
| `ink` | #f4efe6 |
| `accent` | #ff7a45 |
| `accent 2` | #ffd166 |
| `muted` | #c9c1b3 |

## What to change for a new brief

- Wordmark: the `#l1`–`#l10` letter spans (one span per letter keeps the stagger).
- Mark: the three SVG shapes `#m1`–`#m3` (keep `pathLength="100"` so the draw-on works) and the dot `#m4`.
- Tagline: `#tag`. Accent colours: `#ff7a45` and `#ffd166`.

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
