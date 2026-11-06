---
name: motion-year-recap
en_name: "Year in review (9:16)"
en_description: "A fourteen-second vertical year recap for a made-up running app: the year rolls in over scrolling months, distance counts up, three stats and a month chart rise, then a top-4% badge and a sign-off."
zh_name: "年度回顾（竖屏）"
zh_description: "14 秒竖屏年度回顾：年份随滚动的月份出场，总里程上涨，三项数据与月度柱状图依次升起，最后是“前 4%”徽章与告别语。"
description: |
  A 14 s, 9:16 personal year-in-review for apps, clubs and communities: the
  year lands over a scrolling band of months, a big total counts up with a
  progress road, three stat rows and a month chart with the best month lit,
  then a ranking badge and a sign-off. Four scenes; swap any numbers.
triggers:
  - "year in review video"
  - "annual recap"
  - "year recap story"
  - "personal stats recap"
  - "end of year summary"
  - "年度回顾视频"
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
    14-second vertical year in review for a running app on deep purple: "2026
    in review" over scrolling months, 1,284 km counts up, three stat rows and a
    monthly chart with October lit, then a "top 4% of your club" badge.
---

# Year in review (9:16)

A Contour motion template: a 14 s, 1080×1920 (9:16)
HyperFrames composition in one self-contained `example.html`. It is
original Contour work under the Apache-2.0 licence in `LICENSE`. Stridewell and every number in it are made up.

## What is in the film

| Time | Beat |
|---|---|
| 0.2–3.4 s | "2026 in review" lands over a band of months scrolling left. |
| 3.3–6.9 s | "You ran": the total counts to 1,284 km while a road fills. |
| 6.7–10.2 s | Three stat rows slide in, then twelve month bars grow; the best one is lit. |
| 10.0–11.8 s | A "top 4%" badge pops, the sign-off and brand rise. |
| 13.2–13.7 s | The last scene fades for the loop. |

## How it is built

- Every movement is a CSS animation that spans the whole film
  (`--film: 14s`) and loops. Keyframe percentages are fractions of
  the film: 1 s = 7.1429%. To move a beat, change its
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
| `background` | #1b0f3a |
| `ink` | #fff8ec |
| `coral` | #ff7a59 |
| `sky` | #7ce0ff |
| `gold` | #ffd166 |
| `muted` | #b6a9d8 |

## What to change for a new brief

- Year `#year`, the total: `--n: 1284` in the `#km` keyframes, unit and aside line.
- Stat rows `#r1`–`#r3`; month bar heights in the `style` of `#m0`–`#m11` (the `top` one is lit).
- Badge `#badge`, sign-off `#bye`, brand `#brand`.
- Scene colours in the `#bg` keyframes.

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
