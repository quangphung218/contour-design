---
name: motion-cafe-menu
en_name: "Café menu board"
en_description: "A twelve-second menu loop for a made-up café screen: a steaming cup, three pages of drinks and pastries with dotted price leaders, a page bar and a today's-special tag."
zh_name: "咖啡馆菜单屏"
zh_description: "12 秒咖啡馆菜单循环：冒热气的杯子，三页饮品与糕点带点线价格，页码进度条与“今日特饮”标签。"
description: |
  A 12 s, 16:9 menu board loop for café and restaurant screens: a
  chalkboard header with a hand-drawn underline, a cup that keeps
  steaming, three menu pages (four items each, dotted leaders to prices)
  that slide in item by item with a page bar, and a "today's special" tag.
triggers:
  - "menu board video"
  - "cafe menu screen"
  - "digital menu loop"
  - "restaurant menu animation"
  - "coffee shop tv menu"
  - "菜单屏视频"
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
    12-second café menu loop on a dark green board: "Little Ember Café" with
    an orange underline, a steaming cup, three pages (coffee, tea, pastry)
    with prices, a page bar and a "Today's special: maple oat latte" tag.
---

# Café menu board

A Contour motion template: a 12 s, 1920×1080 (16:9)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`. Little Ember Café and its prices are made up.

## What is in the film

| Time | Beat |
|---|---|
| 0–1.5 s | Board, café name, underline and hours come up. |
| 1.0–4.7 s | Page 1 (coffee): four items slide in one by one; the first page bar fills. |
| 4.6–8.3 s | Page 2 (tea and more). |
| 5.6–6.1 s | The special tag pops in under the cup. |
| 8.2–11.8 s | Page 3 (pastry), then everything fades for the loop; the steam rises all along. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 12s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 8.3333%. To move a beat, change its
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
| `board` | #1f2a24 |
| `chalk` | #f2eee3 |
| `accent` | #f4a259 |
| `sage` | #9cc5a1 |
| `muted` | #a9b3a5 |

## What to change for a new brief

- Café name `#title`, hours `#hours`, special `#special`.
- Menu items: the three `.page` blocks (`#p1`–`#p3`), four `.item`s each with a name and a price.
- Page timing: each page's item tracks, its fade and its page bar `#g1`–`#g3` share a start and end.
- Accent `#f4a259`.

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
