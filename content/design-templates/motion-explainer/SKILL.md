---
name: motion-explainer
en_name: "Explainer: how it works"
en_description: "A fourteen-second three-step explainer for a made-up parcel locker service: a dot travels a track, each step pops in with an icon and a one-line caption, then a closing line."
zh_name: "讲解视频：三步说明"
zh_description: "14 秒三步讲解短片：圆点沿轨道前进，每一步带图标弹出并配一句说明，最后给出总结句。"
description: |
  A 14 s, 16:9 explainer for "how it works" pages, onboarding and product
  videos: a title, then a dot travels a track and stops at three steps;
  each step pops in with a line icon, a short name and one caption, and a
  closing line sums it up. Keep three steps or add a fourth node.
triggers:
  - "explainer video"
  - "how it works video"
  - "three step explainer"
  - "process animation"
  - "onboarding video"
  - "讲解视频"
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
    14-second explainer "How Parcelo works" on pale blue: a dot travels a track
    past three steps (order, we drop it in a locker, open it with your phone),
    each with a line icon and caption, then "Three steps. No queues."
---

# Explainer: how it works

A Contour motion template: a 14 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`. Parcelo is a made-up service.

## What is in the film

| Time | Beat |
|---|---|
| 0.3–1.7 s | Title drops in; the track draws. |
| 1.6–3.0 s | Step 1 node pops in under the dot, caption rises. |
| 3.6–6.2 s | The dot travels to step 2, the track fills behind it. |
| 7.0–9.6 s | On to step 3. |
| 10.4–13.4 s | The title gives way to the closing line; all fades for the loop. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 14s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 7.1429%. To move a beat, change its
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
| `background` | #eef3ff |
| `ink` | #14213d |
| `accent` | #ff8a3d |
| `highlight` | #3d5afe |
| `node` | #ffffff |
| `muted` | #5c6b8a |

## What to change for a new brief

- Title `#title`, closing line `#outro`.
- Each step: icon inside `#n1`–`#n3` (line icons in a 100×100 viewBox), caption `#c1`–`#c3`.
- Step spacing: nodes sit at 18.75, 50 and 81.25cqw; the dot moves 31.25cqw per step.
- Accent `#ff8a3d` and highlight `#3d5afe`.

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
