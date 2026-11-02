---
name: motion-transition-pack
en_name: "Transition pack"
en_description: "A nine-second reel of four scene transitions — diagonal wipe, iris, slats and push — each labelled, to lift into any edit."
description: |
  A 9 s reel of four scene-to-scene transitions built from CSS
  clip-paths and transforms: diagonal wipe, circle iris, staggered slats and
  a push. Each scene is labelled, so the film doubles as a reference; lift
  the keyframes of one transition into another film.
triggers:
  - "transition pack"
  - "scene transitions"
  - "wipe transition"
  - "video transitions"
  - "iris transition"
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
    9-second transition reel: five flat colour scenes labelled 01–05,
    joined by a diagonal wipe, a circle iris, slats and a push left.
---

# Transition pack

A Contour motion template: a 9 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`.

## What is in the film

| Time | Beat |
|---|---|
| 0.2 s | Scene 01 label rises. |
| 1.6–2.3 s | Diagonal wipe to scene 02 (`clip-path: polygon`). |
| 3.4–4.1 s | Circle iris opens on scene 03 (`clip-path: circle`). |
| 5.1–6.1 s | Six slats sweep across and retract, revealing scene 04. |
| 7.1–7.7 s | Push left to scene 05. |

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
| `scene 1` | #f25c54 |
| `scene 2` | #f7b267 |
| `scene 3` | #4ecdc4 |
| `scene 4` | #5a67d8 |
| `scene 5` | #1f2933 |

## What to change for a new brief

- Scene names `#m1`–`#m5` and colours (inline `background` / `color` on each `.scene`).
- Each transition lives in one scene's keyframes: wipe = `#s2` `clip-path: polygon`, iris = `#s3` `clip-path: circle`, slats = `#sl1`–`#sl6` then `#s4`, push = `#s4` and `#s5` `translateX`.
- Transitions are 0.6–0.8 s; keep each scene on screen at least 1.2 s so it reads.

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
