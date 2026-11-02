---
name: motion-kinetic-type
en_name: "Kinetic typography"
en_description: "An eight-second type-only film: short phrases slam, slide and swap on a cream page, ending on one bold line."
description: |
  An 8 s kinetic typography film: four short phrases that slam, slide,
  stretch and swap on a cream page, ending on one bold statement line. Type
  only, no imagery. Replace the phrases and keep one idea per beat.
triggers:
  - "kinetic typography"
  - "kinetic type"
  - "type animation"
  - "text animation"
  - "lyric video"
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
    8-second kinetic typography on a cream page: "Make it" / "move." then
    "Make it" / "matter." then the end line "Ideas deserve motion." in red.
---

# Kinetic typography

A Contour motion template: a 8 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0.2–1.1 s | "Make it" whips in from the left; "move." slams down from 2.4× scale. |
| 2.4 s | The pair splits up and down out of frame. |
| 2.7–3.6 s | "Make it" drops in; "matter." stretches open from its left edge. |
| 5.1–5.8 s | The end line focuses in from a blur and holds; it fades at 7.4 s. |
| 0–8 s | A progress bar fills along the bottom edge. |

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
| `background` | #f3ede2 |
| `ink` | #1b1a17 |
| `accent` | #e4472b |

## What to change for a new brief

- Phrases: `#a1`/`#a2`, `#b1`/`#b2` and the end line `#c1`. Keep each under ~10 characters so the 230px type fits.
- Accent: `#e4472b` (the second word of each pair and the italic end word).
- The bottom bar `#bar` is the film's progress; leave its timing alone when you retime beats.

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
