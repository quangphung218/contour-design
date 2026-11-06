---
name: motion-app-feature-tour
en_name: "App feature tour (9:16)"
en_description: "A twelve-second vertical tour of a made-up plant care app: a phone rises, three screens slide past with a headline for each feature, then a green end card."
zh_name: "App 功能导览（竖屏）"
zh_description: "12 秒竖屏 App 功能导览：手机升起，三屏依次滑过，每屏配一句功能标题，最后落下品牌结尾卡。"
description: |
  A 12 s, 9:16 app feature tour for store previews, stories and launch posts:
  a phone rises, its screen slides through three feature screens while a
  numbered headline above changes with each one and page dots follow,
  then a brand panel wipes up with the app name and where to get it.
triggers:
  - "app feature video"
  - "app promo vertical"
  - "app store preview"
  - "feature tour"
  - "mobile app walkthrough"
  - "App 功能介绍视频"
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
    12-second vertical tour of a plant care app on pale green: the phone rises,
    three screens (watering reminders, growth chart, shared shelf) slide past
    with a headline each, then a green end card with the app name.
---

# App feature tour (9:16)

A Contour motion template: a 12 s, 1080×1920 (9:16)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`. Mossday is a made-up app.

## What is in the film

| Time | Beat |
|---|---|
| 0.2–1.2 s | The phone rises in; the first headline lands. |
| 3.5–4.1 s | Screen 2 slides in, headline and page dot change; its bars grow. |
| 6.9–7.5 s | Screen 3 slides in. |
| 9.4–10.2 s | The phone settles back and the green end card wipes up. |
| 11.4–11.9 s | The end card wipes away for the loop. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 12s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 8.3333%. To move a beat, change its
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
| `background` | #e9f1e7 |
| `ink` | #17301f |
| `accent` | #2f8f5b |
| `sun` | #f2b33d |
| `screen` | #fbfdf9 |
| `muted` | #5e7465 |

## What to change for a new brief

- Headlines `#t1`–`#t3` (a short label in `<b>`, the line in `<strong>`).
- Screen content: the three `.s` blocks inside `#strip`; draw with plain shapes, no images.
- End card `#end`: mark, name and store line.
- Accent `#2f8f5b`.

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
