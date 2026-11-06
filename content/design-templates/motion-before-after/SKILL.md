---
name: motion-before-after
en_name: "Before and after"
en_description: "A ten-second comparison: a cluttered form and a clean one share a frame, a slider wipes from before to after and two result pills land."
zh_name: "前后对比"
zh_description: "10 秒前后对比短片：杂乱表单与简洁表单同框，滑块从“之前”扫到“之后”，最后弹出两枚结果标签。"
description: |
  A 10 s, 16:9 before/after film for redesigns, makeovers and product
  upgrades: one frame holds both states, a slider handle wipes the "after"
  over the "before", pauses half way, finishes, and two result pills show
  what changed. Replace both panes with any drawing or screenshot-free mock.
triggers:
  - "before and after video"
  - "comparison slider"
  - "redesign reveal"
  - "makeover video"
  - "old vs new"
  - "前后对比视频"
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
    10-second before/after for a form redesign on warm off-white: a cluttered
    grey form, a slider wipes to a clean three-field form with a violet button,
    then pills "Fields 14 → 3" and "Time to finish 3 min 40 s → 58 s".
---

# Before and after

A Contour motion template: a 10 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`. The form and its numbers are made up.

## What is in the film

| Time | Beat |
|---|---|
| 0.3–1.4 s | Title and the frame ("Before" pane) rise in. |
| 2.0–3.4 s | The handle wipes the "After" pane over half the frame. |
| 3.4–4.6 s | Hold at half: both states side by side. |
| 4.6–6.4 s | The wipe finishes and the handle fades. |
| 6.4–9.8 s | Two result pills rise in, then everything fades for the loop. |

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
| `background` | #f3f0ea |
| `ink` | #1d1d1f |
| `after` | #5b4bff |
| `before` | #c9573a |
| `before pane` | #e4dfd6 |
| `muted` | #77736b |

## What to change for a new brief

- The two panes: `.before` (here grey bars and red dots) and `#after` (fields and a button).
- Labels `.lb`/`.la`, title `#title`, result pills `#s1`/`#s2`.
- Wipe stops: `#after` `clip-path` and `#handle` `left` move together (the frame spans 16.6667cqw–83.3333cqw).
- Accent `#5b4bff`.

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
