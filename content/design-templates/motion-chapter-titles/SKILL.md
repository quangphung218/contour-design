---
name: motion-chapter-titles
en_name: "Chapter title cards"
en_description: "A nine-second set of three editorial chapter cards: big numerals roll up, a rule draws and serif titles settle, joined by soft slides."
description: |
  A 9 s set of three chapter cards for explainers, documentaries and
  long talks: a big numeral rolls up, a rule draws across, a serif title
  and a one-line subtitle settle, then the next chapter slides in. Use one
  card per chapter or all three as a contents sequence.
triggers:
  - "chapter title"
  - "chapter card"
  - "section title"
  - "title card"
  - "documentary titles"
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
    9-second set of three editorial chapter cards on bone paper: "01 The
    problem", "02 The idea", "03 The result", serif titles, rust numerals.
---

# Chapter title cards

A Contour motion template: a 9 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0.25–1.6 s | "01" rolls up, the rule draws, "The problem" rises, the subtitle fades in. |
| 2.9–4.6 s | Chapter 02 slides in from the right and builds the same way. |
| 5.9–7.6 s | Chapter 03 slides in and builds. |
| 8.4 s | Fade to paper. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 9s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 11.111%. To move a beat, change its
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
| `paper` | #eee8dc |
| `ink` | #22201c |
| `numerals` | #b5532f |
| `muted` | #6d655a |

## What to change for a new brief

- Chapters: numeral `#noN`, title `#tN`, subtitle `#sN` for N = 1–3.
- One chapter per film: delete `#c2`/`#c3` and their keyframes, then set `--film` and `data-duration` to 3 s.
- Rust accent `#b5532f`; ink `#22201c` on bone paper `#eee8dc`.

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
