---
name: motion-end-screen
en_name: "End screen"
en_description: "An eight-second outro: a thank-you line, two next-video slots and a subscribe button slide in, then hold for clickable overlays."
description: |
  An 8 s end screen (outro) for long-form videos: a thank-you line, a
  round avatar slot, two "watch next" slots and a subscribe button slide in
  and hold, leaving space for a platform's clickable end-screen overlays.
  Replace the copy and slot titles.
triggers:
  - "end screen"
  - "outro"
  - "video outro"
  - "end card"
  - "subscribe screen"
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
    8-second end screen on midnight blue: "Thanks for watching", avatar
    slot, two "Watch next" slots titled "Part 2: Wiring it up" and "Tools I
    use", a coral subscribe button.
---

# End screen

A Contour motion template: a 8 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0.2–0.9 s | "Thanks for watching" drops in. |
| 0.7–1.9 s | Avatar slot pops; the subscribe button rises; two watch-next slots slide in from the right. |
| 2.4–2.9 s | The subscribe button gives one pulse. |
| 2.9–7.4 s | Hold for the platform's clickable overlays; fade out at 7.4 s. |

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
| `background` | #121a33 |
| `ink` | #f2f4ff |
| `accent` | #ff6f61 |
| `slots` | #1d2850 / #4d5c96 |
| `muted` | #8d98c8 |

## What to change for a new brief

- Heading `#h`, button `#sub`, slot titles inside `#v1` / `#v2`.
- The dashed avatar and the thumbnails are placeholders for the platform's own end-screen elements; keep them where the overlays will sit.
- Accent: `#ff6f61`.

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
