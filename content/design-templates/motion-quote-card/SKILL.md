---
name: motion-quote-card
en_name: "Animated quote card"
en_description: "An eight-second square quote: a large quote mark draws in, the words appear one by one and the attribution slides up."
description: |
  An 8 s square (1:1) quote card for social feeds: a large quote mark
  draws in, the quote appears word by word, a rule opens and the
  attribution slides up. Replace the quote (≈ 6–14 words) and the name.
triggers:
  - "quote card"
  - "animated quote"
  - "quote video"
  - "testimonial video"
  - "square video"
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
    8-second square quote card, deep green on paper: "Good design is a
    quiet promise, kept every single day." — Ana Ruiz, studio founder.
---

# Animated quote card

A Contour motion template: a 8 s, 1080×1080 (1:1)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0.2–1.4 s | The two halves of the quote mark draw on. |
| 1.4–3.4 s | The quote appears word by word (0.16 s apart). |
| 3.4–4.3 s | A short rule opens; the name and role slide up. |
| 7.3 s | Everything fades to paper. |

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
| `paper` | #f1ece0 |
| `ink` | #173a2f |
| `accent` | #c4572e |
| `muted` | #4f6b61 |

## What to change for a new brief

- Quote: the `#q1`… word spans (one per word keeps the reveal). Add or remove keyframe blocks to match the word count.
- Attribution `#who` and the corner tag `#corner`.
- Accent: `#c4572e` (quote mark and rule).

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
