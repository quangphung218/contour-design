---
name: motion-channel-intro
en_name: "Channel intro"
en_description: "A seven-second channel opener: a grid of colour tiles flips in, clears to the channel name and a weekly tagline, then a logo dot bounces."
description: |
  A 7 s intro for a video channel, podcast or series: a 4×4 grid of
  colour tiles flips in and clears away, the channel name lands with a
  bouncing logo dot, and a schedule tagline fades up. Replace the name,
  tagline and tile colours.
triggers:
  - "channel intro"
  - "youtube intro"
  - "video intro"
  - "series opener"
  - "podcast intro"
  - "show open"
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
    7-second channel intro: a grid of bright tiles flips in and clears,
    channel name "The Workbench" with a bouncing yellow dot, tagline "Build
    notes · every Thursday". Charcoal background.
---

# Channel intro

A Contour motion template: a 7 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0.1–1.0 s | Sixteen tiles flip in on a diagonal wave. |
| 1.6–2.5 s | The tiles flip away on the same wave, revealing the dark stage. |
| 2.3–4.1 s | The channel name rises; the dot drops in and bounces three times. |
| 3.6–4.5 s | Tagline and episode label fade up; all of it eases out at 6.3 s. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 7s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 14.286%. To move a beat, change its
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
| `background` | #16181d |
| `ink` | #f7f5f0 |
| `dot` | #ffb703 |
| `tiles` | #2f6df6, #ffb703, #fb5607, #06d6a0… |

## What to change for a new brief

- Channel name: `#n1` and `#n2`; tagline `#tag`; episode label `#ep`.
- Tile colours: the inline `background` on the 16 `.grid i` tiles.
- Logo dot `#dot` (`#ffb703`): swap for a small inline SVG mark if the channel has one.

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
