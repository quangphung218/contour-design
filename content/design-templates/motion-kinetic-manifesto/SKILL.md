---
name: motion-kinetic-manifesto
en_name: "Kinetic manifesto (9:16)"
en_description: "A ten-second vertical type film: a four-word line stacks up, single words punch in one per beat, then a lime panel wipes up with a two-line motto and a signature."
zh_name: "动态文字宣言（竖屏）"
zh_description: "10 秒竖屏文字动画：四个词依次叠起，单词随节拍逐个冲出，随后荧光绿面板上推，落下两行口号与署名。"
description: |
  A 10 s, 9:16 kinetic typography film for vertical feeds and stories: a
  four-word statement stacks up line by line, then single words punch in
  on the beat, and a bright panel wipes up for a two-line motto and a
  signature. Type only; change the words, keep the rhythm.
triggers:
  - "kinetic typography vertical"
  - "manifesto video"
  - "text animation reel"
  - "typography story"
  - "words on beat"
  - "竖屏文字动画"
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
    10-second vertical kinetic type on black: "Make things people keep." stacks
    up, then "Small / teams / ship / fast" punch in one per beat, then a lime
    panel wipes up with "Ship small. Ship often." and a studio signature.
---

# Kinetic manifesto (9:16)

A Contour motion template: a 10 s, 1080×1920 (9:16)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`. Tallowmark is a made-up studio.

## What is in the film

| Time | Beat |
|---|---|
| 0.4–2.1 s | Four words slam up into a left-aligned stack (one outlined, one lime). |
| 3.0–3.4 s | The stack flies up and out. |
| 3.5–6.3 s | Four single words punch in, one every 0.7 s. |
| 6.2–6.6 s | A lime panel wipes up from the bottom. |
| 6.7–9.9 s | Two-line motto, signature and bar; the panel wipes away for the loop. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 10s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 10%. To move a beat, change its
  percentage; to change the length, change `--film`, `data-duration` and
  the percentages together.
- `window.__timelines.main` is the seekable master timeline. Its `seek(t)`
  pins every CSS animation to `t`, so the render is the same every time.
  Keep it when you edit; add new motion as CSS animations, not timers.
- Sizes are in `cqw` (1cqw = 10.8px on the 1080px grid), so the stage
  scales to the preview and renders at exactly 1080×1920.
- System fonts and inline SVG only. Nothing is downloaded, no images, no
  web fonts, no CDN scripts.
- Sound: none. The film declares `od-motion-audio` `silent`. If the
  brief asks for sound, add a local `<audio>` track the motion-design skill
  describes, and say where it came from.

## Palette

| Token | Value |
|---|---|
| `background` | #111111 |
| `ink` | #f5f5f0 |
| `accent` | #d4ff3a |

## What to change for a new brief

- The stacked words `#w1`–`#w4` (keep them short: 4–7 letters fit at 20.3704cqw).
- The beat words `#b1`–`#b4`; to add one, copy a `punch` track 0.7 s later.
- Motto `#e1`/`#e2` and signature `#sign`.
- Accent `#d4ff3a`.

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
